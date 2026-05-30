use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::json;

use crate::{
    audio,
    cli::OnAirArgs,
    config::{self, ResolvedConfig},
    downstream::DownstreamClient,
    ffmpeg,
    ledger::Ledger,
    renderer, scenario,
    upstream::{EpisodeIndexItem, UpstreamClient},
};

pub async fn run(args: OnAirArgs) -> Result<()> {
    let mut loaded_config = config::load(args.config.as_deref())?;
    loaded_config
        .values
        .apply_overrides(args.config_overrides());
    loaded_config.values.validate()?;

    let upstream_url = loaded_config
        .values
        .upstream_episode_url
        .as_deref()
        .context("onair には [upstream].episode_url が必要です")?;
    let upstream = UpstreamClient::new(resolve_upstream_access_token(&loaded_config.values));

    println!("Discovering upstream episodes: {upstream_url}");
    let episodes = upstream.list_episodes(upstream_url).await?;
    println!("Discovered episodes: {}", episodes.len());

    if args.dry_run {
        let mut candidates = episodes;
        if let Some(limit) = args.limit {
            candidates.truncate(limit);
        }

        println!("Dry run episodes: {}", candidates.len());
        for episode in &candidates {
            println!("- {}", episode.episode_key);
        }
        println!("Dry run: discovery only");
        return Ok(());
    }

    let ledger = Ledger::open(&audio::absolute_path(&loaded_config.values.onair_database)?)?;
    let uploaded = ledger
        .uploaded_episode_keys()?
        .into_iter()
        .collect::<HashSet<_>>();
    let mut candidates = episodes
        .into_iter()
        .filter(|episode| !uploaded.contains(&episode.episode_key))
        .collect::<Vec<_>>();

    if let Some(limit) = args.limit {
        candidates.truncate(limit);
    }

    println!("Unprocessed episodes: {}", candidates.len());
    for episode in &candidates {
        println!("- {}", episode.episode_key);
    }

    let upload_url = loaded_config
        .values
        .downstream_upload_url
        .as_deref()
        .context("onair には [downstream].upload_url が必要です")?
        .to_string();

    ffmpeg::ensure_available()?;
    let downstream = DownstreamClient::new(resolve_downstream_access_token(&loaded_config.values));

    for episode in candidates {
        match process_episode(
            &loaded_config.values,
            &upstream,
            &downstream,
            upstream_url,
            &upload_url,
            &ledger,
            &episode,
        )
        .await
        {
            Ok(()) => println!("Uploaded: {}", episode.episode_key),
            Err(error) => {
                println!("Failed: {}: {error:#}", episode.episode_key);
                ledger.mark_failed(&episode.episode_key, &format!("{error:#}"))?;
            }
        }
    }

    println!("Onair done");

    Ok(())
}

async fn process_episode(
    config: &ResolvedConfig,
    upstream: &UpstreamClient,
    downstream: &DownstreamClient,
    upstream_url: &str,
    upload_url: &str,
    ledger: &Ledger,
    episode: &EpisodeIndexItem,
) -> Result<()> {
    println!("Processing: {}", episode.episode_key);
    ledger.upsert_pending(&episode.episode_key)?;

    let detail_url = episode_detail_url(upstream_url, &episode.episode_key);
    println!("Downloading Episode JSON: {detail_url}");
    let raw_json = upstream.fetch_episode_json(&detail_url).await?;
    let scenario = scenario::parse(&raw_json).context("Episode JSON を解析できません")?;
    scenario::validate(&scenario)?;

    let episode_dir = onair_episode_dir(config, &episode.episode_key)?;
    fs::create_dir_all(&episode_dir).with_context(|| {
        format!(
            "onair episode ディレクトリを作成できません: {}",
            episode_dir.display()
        )
    })?;

    let json_path = episode_dir.join("episode.json");
    write_file(&json_path, raw_json.as_bytes(), "Episode JSON")?;
    ledger.mark_fetched(&episode.episode_key, &json_path)?;

    let audio_path = episode_dir.join("audio.mp3");
    let workdir = onair_work_dir(config, &episode.episode_key)?;
    println!("Recording MP3: {}", audio_path.display());
    renderer::render_scenario_to_mp3(config, &scenario, audio_path.clone(), workdir).await?;
    ledger.mark_recorded(&episode.episode_key, &audio_path)?;

    let render_metadata_path = episode_dir.join("render_metadata.json");
    write_render_metadata(
        config,
        &episode.episode_key,
        &json_path,
        &audio_path,
        &render_metadata_path,
    )?;

    println!("Uploading downstream: {upload_url}");
    downstream
        .upload_episode(upload_url, &episode.episode_key, &json_path, &audio_path)
        .await?;
    ledger.mark_uploaded(&episode.episode_key)?;

    Ok(())
}

fn resolve_upstream_access_token(config: &ResolvedConfig) -> Option<String> {
    std::env::var("VOICEPIPE_UPSTREAM_ACCESS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| config.upstream_access_token.clone())
}

fn resolve_downstream_access_token(config: &ResolvedConfig) -> Option<String> {
    std::env::var("VOICEPIPE_DOWNSTREAM_ACCESS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| config.downstream_access_token.clone())
}

fn episode_detail_url(index_url: &str, episode_key: &str) -> String {
    format!("{}/{}", index_url.trim_end_matches('/'), episode_key)
}

fn onair_episode_dir(config: &ResolvedConfig, episode_key: &str) -> Result<PathBuf> {
    let directory = audio::safe_file_component(episode_key, "episode");
    audio::absolute_path(&config.onair_episodes_dir.join(directory))
}

fn onair_work_dir(config: &ResolvedConfig, episode_key: &str) -> Result<PathBuf> {
    let directory = audio::safe_file_component(episode_key, "episode");
    audio::absolute_path(&config.onair_work_dir.join(directory))
}

fn write_file(path: &Path, bytes: &[u8], label: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!("{label} ディレクトリを作成できません: {}", parent.display())
        })?;
    }

    fs::write(path, bytes).with_context(|| format!("{label} を保存できません: {}", path.display()))
}

fn write_render_metadata(
    config: &ResolvedConfig,
    episode_key: &str,
    json_path: &Path,
    audio_path: &Path,
    metadata_path: &Path,
) -> Result<()> {
    let generated_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("現在時刻を取得できません")?
        .as_secs();
    let metadata = json!({
        "episode_key": episode_key,
        "generated_at_unix_seconds": generated_at_unix_seconds,
        "voicepipe_version": env!("CARGO_PKG_VERSION"),
        "json_path": json_path.display().to_string(),
        "audio_path": audio_path.display().to_string(),
        "voicevox_endpoint": config.voicevox_endpoint,
        "speaker": config.speaker,
        "voice": {
            "speed_scale": config.voice.speed_scale,
            "pitch_scale": config.voice.pitch_scale,
            "intonation_scale": config.voice.intonation_scale,
            "pause_length_scale": config.voice.pause_length_scale,
            "volume_scale": config.voice.volume_scale
        },
        "audio": {
            "bitrate": config.bitrate,
            "format": config.format
        }
    });
    let content =
        serde_json::to_vec_pretty(&metadata).context("render metadata を JSON 化できません")?;

    write_file(metadata_path, &content, "render metadata")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_detail_url_appends_episode_key() {
        assert_eq!(
            episode_detail_url("https://example.com/api/episodes/", "episode-001"),
            "https://example.com/api/episodes/episode-001"
        );
    }

    #[test]
    fn storage_paths_use_configured_directories() {
        let config = ResolvedConfig {
            onair_episodes_dir: PathBuf::from("custom/dist/onair/episodes"),
            onair_work_dir: PathBuf::from("custom/work/onair"),
            ..ResolvedConfig::default()
        };

        assert!(
            onair_episode_dir(&config, "episode-001")
                .expect("episode dir")
                .ends_with("custom/dist/onair/episodes/episode-001")
        );
        assert!(
            onair_work_dir(&config, "episode-001")
                .expect("work dir")
                .ends_with("custom/work/onair/episode-001")
        );
    }
}
