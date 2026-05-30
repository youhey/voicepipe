use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

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

    let ledger = Ledger::open(&audio::absolute_path(
        &loaded_config.values.storage_database,
    )?)?;
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
    let downstream = DownstreamClient::new();

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

    let json_path = storage_json_path(config, &episode.episode_key)?;
    write_file(&json_path, raw_json.as_bytes(), "Episode JSON")?;
    ledger.mark_fetched(&episode.episode_key, &json_path)?;

    let audio_path = storage_audio_path(config, &episode.episode_key)?;
    let workdir = audio::default_workdir(&episode.episode_key)?;
    println!("Recording MP3: {}", audio_path.display());
    renderer::render_scenario_to_mp3(config, &scenario, audio_path.clone(), workdir).await?;
    ledger.mark_recorded(&episode.episode_key, &audio_path)?;

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

fn episode_detail_url(index_url: &str, episode_key: &str) -> String {
    format!("{}/{}", index_url.trim_end_matches('/'), episode_key)
}

fn storage_json_path(config: &ResolvedConfig, episode_key: &str) -> Result<PathBuf> {
    let filename = format!(
        "{}.json",
        audio::safe_file_component(episode_key, "episode")
    );
    audio::absolute_path(&config.storage_json_dir.join(filename))
}

fn storage_audio_path(config: &ResolvedConfig, episode_key: &str) -> Result<PathBuf> {
    let filename = format!("{}.mp3", audio::safe_file_component(episode_key, "episode"));
    audio::absolute_path(&config.storage_audio_dir.join(filename))
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
            storage_json_dir: PathBuf::from("custom/json"),
            storage_audio_dir: PathBuf::from("custom/audio"),
            ..ResolvedConfig::default()
        };

        assert!(
            storage_json_path(&config, "episode-001")
                .expect("json path")
                .ends_with("custom/json/episode-001.json")
        );
        assert!(
            storage_audio_path(&config, "episode-001")
                .expect("audio path")
                .ends_with("custom/audio/episode-001.mp3")
        );
    }
}
