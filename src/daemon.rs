use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tokio::{sync::Notify, time::sleep};

use crate::{
    audio,
    cli::DaemonArgs,
    config::{self, KeepAliveConfig, ResolvedConfig},
    ffmpeg, onair,
    upstream::UpstreamClient,
    voicevox::VoicevoxClient,
};

pub async fn run(args: DaemonArgs) -> Result<()> {
    let loaded_config = config::load(args.config.as_deref())?;
    loaded_config.values.validate()?;
    let interval = args
        .interval
        .unwrap_or(loaded_config.values.daemon.interval);
    if interval == 0 {
        bail!("--interval は 1 以上を指定してください");
    }

    validate_environment(&loaded_config.values).await?;

    println!("voicepipe daemon started");
    println!("interval={interval}");

    let shutdown = Shutdown::new();
    shutdown.listen();
    let keepalive_handle =
        start_keepalive(loaded_config.values.keepalive.clone(), shutdown.clone())?;

    loop {
        println!("running onair cycle...");
        if let Err(error) = onair::run_onair_once(args.to_onair_args()).await {
            println!("onair cycle failed: {error:#}");
        }

        if args.once || shutdown.is_requested() {
            break;
        }

        println!("sleeping {interval} seconds");
        tokio::select! {
            () = sleep(Duration::from_secs(interval)) => {}
            () = shutdown.notified() => {
                break;
            }
        }
    }

    println!("voicepipe daemon stopped");
    if let Some(handle) = keepalive_handle {
        handle.abort();
    }

    Ok(())
}

fn start_keepalive(
    config: KeepAliveConfig,
    shutdown: Shutdown,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    if !config.enabled || config.urls.is_empty() {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout))
        .build()
        .context("keepalive HTTP client を作成できません")?;

    println!("keepalive started");
    println!("keepalive_interval={}", config.interval);

    Ok(Some(tokio::spawn(async move {
        loop {
            run_keepalive_once(&client, &config.urls).await;

            tokio::select! {
                () = sleep(Duration::from_secs(config.interval)) => {}
                () = shutdown.notified() => {
                    break;
                }
            }

            if shutdown.is_requested() {
                break;
            }
        }

        println!("keepalive stopped");
    })))
}

async fn run_keepalive_once(client: &reqwest::Client, urls: &[String]) {
    for url in urls {
        match client.get(url).send().await {
            Ok(response)
                if response.status().is_success() || response.status().is_redirection() =>
            {
                println!("keepalive ok: {url}");
            }
            Ok(response) => {
                println!(
                    "warning: keepalive failed: {url}: HTTP {}",
                    response.status()
                );
            }
            Err(error) => {
                println!("warning: keepalive failed: {url}: {error}");
            }
        }
    }
}

async fn validate_environment(config: &ResolvedConfig) -> Result<()> {
    let upstream_url = config
        .upstream_episode_url
        .as_deref()
        .context("daemon には [upstream].episode_url が必要です")?;

    ffmpeg::ensure_available()?;
    ffmpeg::ensure_probe_available()?;

    let voicevox = VoicevoxClient::new(
        config.voicevox_endpoint.clone(),
        config.speaker,
        config.voice.clone(),
    );
    if let Err(error) = voicevox.ensure_ready().await {
        println!("warning: daemon 起動前の VOICEVOX 到達確認に失敗しました: {error:#}");
    }

    let upstream = UpstreamClient::new(resolve_upstream_access_token(config));
    if let Err(error) = upstream.list_episodes(upstream_url).await {
        println!("warning: daemon 起動前の upstream 到達確認に失敗しました: {error:#}");
    }

    ensure_sqlite_writable(&config.onair_database)?;
    ensure_directory_writable(Path::new("dist"))?;
    ensure_directory_writable(Path::new("work"))?;

    Ok(())
}

fn resolve_upstream_access_token(config: &ResolvedConfig) -> Option<String> {
    std::env::var("VOICEPIPE_UPSTREAM_ACCESS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| config.upstream_access_token.clone())
}

fn ensure_sqlite_writable(path: &Path) -> Result<()> {
    let absolute = audio::absolute_path(path)?;
    if let Some(parent) = absolute.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "SQLite データベースディレクトリを作成できません: {}",
                parent.display()
            )
        })?;
    }

    let connection = rusqlite::Connection::open(&absolute).with_context(|| {
        format!(
            "SQLite データベースを書き込み用に開けません: {}",
            absolute.display()
        )
    })?;
    connection
        .execute_batch("CREATE TABLE IF NOT EXISTS daemon_writable_check (checked_at TEXT); DROP TABLE daemon_writable_check;")
        .with_context(|| format!("SQLite データベースに書き込めません: {}", absolute.display()))?;

    Ok(())
}

fn ensure_directory_writable(path: &Path) -> Result<()> {
    let absolute = audio::absolute_path(path)?;
    fs::create_dir_all(&absolute)
        .with_context(|| format!("ディレクトリを作成できません: {}", absolute.display()))?;

    let test_file = absolute.join(".voicepipe-daemon-write-test");
    fs::write(&test_file, b"voicepipe daemon\n")
        .with_context(|| format!("テストファイルを書き込めません: {}", test_file.display()))?;
    fs::remove_file(&test_file)
        .with_context(|| format!("テストファイルを削除できません: {}", test_file.display()))?;

    Ok(())
}

#[derive(Clone)]
struct Shutdown {
    requested: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Shutdown {
    fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    fn listen(&self) {
        let shutdown = self.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                println!("shutdown requested");
                shutdown.requested.store(true, Ordering::SeqCst);
                shutdown.notify.notify_waiters();
            }
        });
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    async fn notified(&self) {
        self.notify.notified().await;
    }
}
