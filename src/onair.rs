use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    artifact::{self, ArtifactMetadata},
    audio,
    cli::OnAirArgs,
    config::{self, ResolvedConfig},
    downstream::{DownstreamClient, DownstreamManifestEpisode, SyncEpisodeRequest, UploadMethod},
    ffmpeg,
    ledger::{DownstreamSyncMetadata, Ledger},
    renderer, scenario,
    upstream::{EpisodeIndexItem, UpstreamClient},
};

pub async fn run(args: OnAirArgs) -> Result<()> {
    run_onair_once(args).await
}

pub async fn run_onair_once(args: OnAirArgs) -> Result<()> {
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

    let upload_url = loaded_config
        .values
        .downstream_upload_url
        .as_deref()
        .context("onair には [downstream].upload_url が必要です")?
        .to_string();
    let downstream = DownstreamClient::new(resolve_downstream_access_token(&loaded_config.values));

    println!("Fetching downstream manifest...");
    let manifest = downstream.fetch_manifest(&upload_url).await?;
    println!("Downstream manifest episodes: {}", manifest.len());
    let manifest_by_episode_key = manifest
        .into_iter()
        .map(|episode| (episode.episode_key.clone(), episode))
        .collect::<HashMap<_, _>>();

    let ledger = Ledger::open(&audio::absolute_path(&loaded_config.values.onair_database)?)?;
    let mut candidates = episodes;

    if let Some(limit) = args.limit {
        candidates.truncate(limit);
    }

    println!("Candidate episodes: {}", candidates.len());
    for episode in &candidates {
        println!("- {}", episode.episode_key);
    }

    ffmpeg::ensure_available()?;
    ffmpeg::ensure_probe_available()?;

    for episode in candidates {
        let context = ProcessEpisodeContext {
            config: &loaded_config.values,
            upstream: &upstream,
            downstream: &downstream,
            upstream_url,
            upload_url: &upload_url,
            ledger: &ledger,
            downstream_state: manifest_by_episode_key.get(&episode.episode_key),
        };
        match process_episode(&context, &episode).await {
            Ok(()) => println!("Processed: {}", episode.episode_key),
            Err(error) => {
                println!("Failed: {}: {error:#}", episode.episode_key);
                ledger.mark_failed(&episode.episode_key, &format!("{error:#}"))?;
            }
        }
    }

    println!("Onair done");

    Ok(())
}

struct ProcessEpisodeContext<'a> {
    config: &'a ResolvedConfig,
    upstream: &'a UpstreamClient,
    downstream: &'a DownstreamClient,
    upstream_url: &'a str,
    upload_url: &'a str,
    ledger: &'a Ledger,
    downstream_state: Option<&'a DownstreamManifestEpisode>,
}

async fn process_episode(
    context: &ProcessEpisodeContext<'_>,
    episode: &EpisodeIndexItem,
) -> Result<()> {
    println!("Processing: {}", episode.episode_key);
    context.ledger.upsert_pending(&episode.episode_key)?;

    let detail_url = episode_detail_url(context.upstream_url, &episode.episode_key);
    println!("Downloading Episode JSON: {detail_url}");
    let raw_json = context.upstream.fetch_episode_json(&detail_url).await?;
    let scenario = scenario::parse(&raw_json).context("Episode JSON を解析できません")?;
    scenario::validate(&scenario)?;

    let episode_dir = onair_episode_dir(context.config, &episode.episode_key)?;
    fs::create_dir_all(&episode_dir).with_context(|| {
        format!(
            "onair episode ディレクトリを作成できません: {}",
            episode_dir.display()
        )
    })?;

    let json_path = episode_dir.join("episode.json");
    write_file(&json_path, raw_json.as_bytes(), "Episode JSON")?;
    context
        .ledger
        .mark_fetched(&episode.episode_key, &json_path)?;

    let audio_path = episode_dir.join("audio.mp3");
    let workdir = onair_work_dir(context.config, &episode.episode_key)?;
    println!("Recording MP3: {}", audio_path.display());
    renderer::render_scenario_to_mp3(
        context.config,
        &scenario,
        audio_path.clone(),
        workdir.clone(),
    )
    .await?;
    let audio_duration_seconds = ffmpeg::probe_duration_seconds(&audio_path)?;
    let recorded_at = current_utc_rfc3339()?;
    println!("Recording completed:");
    println!("recorded_at={recorded_at}");
    println!("audio_duration_seconds={audio_duration_seconds}");
    context.ledger.mark_recorded(
        &episode.episode_key,
        &audio_path,
        &recorded_at,
        audio_duration_seconds,
    )?;

    let upload_json_path = workdir.join("episode.upload.json");
    write_upload_episode_json(&raw_json, &scenario, &workdir, &upload_json_path)?;

    let render_metadata_path = episode_dir.join("render_metadata.json");
    write_render_metadata(
        context.config,
        &episode.episode_key,
        &upload_json_path,
        &audio_path,
        &recorded_at,
        audio_duration_seconds,
        &render_metadata_path,
    )?;

    let sync_artifacts = calculate_sync_artifacts(&audio_path, &upload_json_path)?;
    println!("Local artifact metadata:");
    println!("audio_sha256={}", sync_artifacts.audio.sha256);
    println!("episode_json_sha256={}", sync_artifacts.episode_json.sha256);
    println!("audio_size_bytes={}", sync_artifacts.audio.size_bytes);
    println!(
        "episode_json_size_bytes={}",
        sync_artifacts.episode_json.size_bytes
    );

    let decision = decide_downstream_sync(context.downstream_state, &sync_artifacts);
    println!("Downstream state:");
    println!("{}", decision.reason);
    println!("Action: {}", decision.last_upload_method.to_uppercase());

    if let Some(method) = decision.upload_method {
        println!("Uploading episode...");
        context
            .downstream
            .sync_episode(SyncEpisodeRequest {
                method,
                upload_url: context.upload_url,
                episode_key: &episode.episode_key,
                json_path: &upload_json_path,
                audio_path: &audio_path,
                render_metadata_path: &render_metadata_path,
                recorded_at: &recorded_at,
                audio_duration_seconds,
            })
            .await?;
        println!("Upload completed.");
    } else {
        println!("Upload skipped.");
    }

    context.ledger.mark_downstream_synced(
        &episode.episode_key,
        &DownstreamSyncMetadata {
            audio_sha256: sync_artifacts.audio.sha256,
            episode_json_sha256: sync_artifacts.episode_json.sha256,
            audio_size_bytes: sync_artifacts.audio.size_bytes,
            episode_json_size_bytes: sync_artifacts.episode_json.size_bytes,
            downstream_status: decision.downstream_status.to_string(),
            last_upload_method: decision.last_upload_method.to_string(),
        },
        decision.upload_method.is_some(),
    )?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncArtifacts {
    audio: ArtifactMetadata,
    episode_json: ArtifactMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncDecision {
    upload_method: Option<UploadMethod>,
    downstream_status: &'static str,
    last_upload_method: &'static str,
    reason: String,
}

fn calculate_sync_artifacts(audio_path: &Path, upload_json_path: &Path) -> Result<SyncArtifacts> {
    Ok(SyncArtifacts {
        audio: artifact::calculate_metadata(audio_path)?,
        episode_json: artifact::calculate_metadata(upload_json_path)?,
    })
}

fn decide_downstream_sync(
    downstream_state: Option<&DownstreamManifestEpisode>,
    artifacts: &SyncArtifacts,
) -> SyncDecision {
    let Some(downstream_state) = downstream_state else {
        return SyncDecision {
            upload_method: Some(UploadMethod::Post),
            downstream_status: "created",
            last_upload_method: UploadMethod::Post.as_str(),
            reason: "episode is missing from downstream manifest".to_string(),
        };
    };

    let mut mismatches = Vec::new();
    if !optional_hash_matches(
        downstream_state.audio_sha256.as_deref(),
        &artifacts.audio.sha256,
    ) {
        mismatches.push(match downstream_state.audio_sha256.as_deref() {
            Some(_) => "audio hash mismatch",
            None => "audio hash missing",
        });
    }
    if !optional_hash_matches(
        downstream_state.episode_json_sha256.as_deref(),
        &artifacts.episode_json.sha256,
    ) {
        mismatches.push(match downstream_state.episode_json_sha256.as_deref() {
            Some(_) => "episode JSON hash mismatch",
            None => "episode JSON hash missing",
        });
    }

    if mismatches.is_empty() {
        SyncDecision {
            upload_method: None,
            downstream_status: "matched",
            last_upload_method: "skip",
            reason: "already synchronized".to_string(),
        }
    } else {
        SyncDecision {
            upload_method: Some(UploadMethod::Put),
            downstream_status: "repaired",
            last_upload_method: UploadMethod::Put.as_str(),
            reason: mismatches.join(", "),
        }
    }
}

fn optional_hash_matches(downstream_hash: Option<&str>, local_hash: &str) -> bool {
    downstream_hash
        .map(|value| value.eq_ignore_ascii_case(local_hash))
        .unwrap_or(false)
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

fn current_utc_rfc3339() -> Result<String> {
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .context("recorded_at の秒精度変換に失敗しました")?;
    now.format(&Rfc3339)
        .context("recorded_at を RFC3339 形式に変換できません")
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

fn write_upload_episode_json(
    raw_json: &str,
    scenario: &scenario::ScenarioExport,
    workdir: &Path,
    upload_json_path: &Path,
) -> Result<()> {
    let updated_json = update_section_duration_json(raw_json, scenario, |index, section| {
        let wav_path = section_wav_path(workdir, index, &section.section_type);
        ffmpeg::probe_duration_seconds(&wav_path).with_context(|| {
            format!(
                "section WAV の音声長を取得できません: {}",
                wav_path.display()
            )
        })
    })?;

    write_file(
        upload_json_path,
        updated_json.as_bytes(),
        "upload Episode JSON",
    )
}

fn update_section_duration_json<F>(
    raw_json: &str,
    scenario: &scenario::ScenarioExport,
    mut measure_duration: F,
) -> Result<String>
where
    F: FnMut(usize, &scenario::Section) -> Result<u64>,
{
    let mut value = serde_json::from_str::<Value>(raw_json)
        .context("upload 用 Episode JSON を解析できません")?;
    let sections = value
        .pointer_mut("/episode/scenario_json/sections")
        .and_then(Value::as_array_mut)
        .context("upload 用 Episode JSON に episode.scenario_json.sections がありません")?;

    println!("Updating section durations...");
    for (index, section) in scenario.episode.scenario_json.sections.iter().enumerate() {
        let original = section.estimated_duration_seconds;
        match measure_duration(index, section) {
            Ok(actual) => {
                println!("{}:", section.section_type);
                match original {
                    Some(estimated) => println!("estimated={estimated}"),
                    None => println!("estimated=null"),
                }
                println!("actual={actual}");

                if let Some(object) = sections.get_mut(index).and_then(Value::as_object_mut) {
                    object.insert("estimated_duration_seconds".to_string(), json!(actual));
                }
            }
            Err(error) => {
                let section_id = format!("{}_{index:03}", section.section_type);
                let fallback = original
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string());
                println!(
                    "Unable to determine duration for section {section_id}. Keeping original estimated_duration_seconds={fallback}. {error:#}"
                );
            }
        }
    }

    serde_json::to_string_pretty(&value).context("upload 用 Episode JSON を生成できません")
}

fn section_wav_path(workdir: &Path, index: usize, section_type: &str) -> PathBuf {
    workdir.join("segments").join(format!(
        "{:03}_{}.wav",
        index,
        audio::safe_file_component(section_type, "section")
    ))
}

fn write_render_metadata(
    config: &ResolvedConfig,
    episode_key: &str,
    json_path: &Path,
    audio_path: &Path,
    recorded_at: &str,
    audio_duration_seconds: u64,
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
        "recorded_at": recorded_at,
        "audio_duration_seconds": audio_duration_seconds,
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
    use anyhow::anyhow;

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

    #[test]
    fn update_section_duration_json_overwrites_estimates() {
        let raw_json = r#"{
            "episode": {
                "episode_key": "episode-001",
                "title": "テスト回",
                "language": "ja",
                "scenario_json": {
                    "sections": [
                        {
                            "type": "opening",
                            "title": "オープニング",
                            "text": "こんにちは。",
                            "estimated_duration_seconds": 60
                        },
                        {
                            "type": "topic",
                            "title": "トピック",
                            "text": "本文です。",
                            "estimated_duration_seconds": 150
                        }
                    ]
                }
            }
        }"#;
        let scenario = scenario::parse(raw_json).expect("scenario should parse");
        let updated = update_section_duration_json(raw_json, &scenario, |index, _| {
            Ok([57_u64, 143_u64][index])
        })
        .expect("duration json should update");
        let value = serde_json::from_str::<Value>(&updated).expect("updated json should parse");
        let sections = value
            .pointer("/episode/scenario_json/sections")
            .and_then(Value::as_array)
            .expect("sections should exist");

        assert_eq!(sections[0]["estimated_duration_seconds"], json!(57));
        assert_eq!(sections[1]["estimated_duration_seconds"], json!(143));
    }

    #[test]
    fn update_section_duration_json_keeps_original_on_measurement_failure() {
        let raw_json = r#"{
            "episode": {
                "episode_key": "episode-001",
                "title": "テスト回",
                "language": "ja",
                "scenario_json": {
                    "sections": [
                        {
                            "type": "opening",
                            "title": "オープニング",
                            "text": "こんにちは。",
                            "estimated_duration_seconds": 60
                        }
                    ]
                }
            }
        }"#;
        let scenario = scenario::parse(raw_json).expect("scenario should parse");
        let updated = update_section_duration_json(raw_json, &scenario, |_, _| {
            Err(anyhow!("duration unavailable"))
        })
        .expect("duration json should still be generated");
        let value = serde_json::from_str::<Value>(&updated).expect("updated json should parse");

        assert_eq!(
            value["episode"]["scenario_json"]["sections"][0]["estimated_duration_seconds"],
            json!(60)
        );
    }

    #[test]
    fn downstream_sync_decision_posts_when_episode_missing() {
        let artifacts = test_sync_artifacts();

        let decision = decide_downstream_sync(None, &artifacts);

        assert_eq!(decision.upload_method, Some(UploadMethod::Post));
        assert_eq!(decision.last_upload_method, "post");
        assert_eq!(decision.downstream_status, "created");
    }

    #[test]
    fn downstream_sync_decision_skips_when_hashes_match() {
        let artifacts = test_sync_artifacts();
        let downstream = DownstreamManifestEpisode {
            episode_key: "episode-001".to_string(),
            audio_sha256: Some("AUDIO-HASH".to_string()),
            episode_json_sha256: Some("json-hash".to_string()),
            audio_size_bytes: Some(10),
            episode_json_size_bytes: Some(20),
        };

        let decision = decide_downstream_sync(Some(&downstream), &artifacts);

        assert_eq!(decision.upload_method, None);
        assert_eq!(decision.last_upload_method, "skip");
        assert_eq!(decision.downstream_status, "matched");
    }

    #[test]
    fn downstream_sync_decision_puts_when_hashes_differ_or_missing() {
        let artifacts = test_sync_artifacts();
        let downstream = DownstreamManifestEpisode {
            episode_key: "episode-001".to_string(),
            audio_sha256: Some("other-audio-hash".to_string()),
            episode_json_sha256: None,
            audio_size_bytes: Some(10),
            episode_json_size_bytes: Some(20),
        };

        let decision = decide_downstream_sync(Some(&downstream), &artifacts);

        assert_eq!(decision.upload_method, Some(UploadMethod::Put));
        assert_eq!(decision.last_upload_method, "put");
        assert_eq!(decision.downstream_status, "repaired");
        assert!(decision.reason.contains("audio hash mismatch"));
        assert!(decision.reason.contains("episode JSON hash missing"));
    }

    fn test_sync_artifacts() -> SyncArtifacts {
        SyncArtifacts {
            audio: ArtifactMetadata {
                sha256: "audio-hash".to_string(),
                size_bytes: 10,
            },
            episode_json: ArtifactMetadata {
                sha256: "json-hash".to_string(),
                size_bytes: 20,
            },
        }
    }
}
