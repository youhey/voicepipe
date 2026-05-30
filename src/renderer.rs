use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tracing::info;

use crate::{
    audio::{self, DEFAULT_PAUSE_BETWEEN_SECTIONS_MS, RenderPaths},
    cli::{PreviewArgs, RecordArgs, RecordSource, RenderArgs},
    config::{self, ConfigOverrides, ResolvedConfig},
    ffmpeg, scenario,
    scenario::{ScenarioExport, Section},
    upstream::UpstreamClient,
    voicevox::VoicevoxClient,
};

pub async fn record(args: RecordArgs) -> Result<()> {
    let loaded_config = load_effective_config(args.config.as_deref(), args.config_overrides())?;

    let source = load_record_source(&args, &loaded_config.values).await?;
    scenario::validate(&source.scenario)?;

    println!("Episode: {}", source.scenario.episode.episode_key);

    if let Some(output_json) = &args.output_json {
        let output_json = audio::absolute_path(output_json)?;
        write_json_output(&output_json, &source.raw_json)?;
        println!("Saving JSON: {}", output_json.display());
    }

    ffmpeg::ensure_available()?;

    let output = match args.output.as_deref() {
        Some(path) => audio::absolute_path(path)?,
        None => default_record_output(&loaded_config.values, &source.scenario.episode.episode_key)?,
    };
    let workdir = match args.workdir.as_deref() {
        Some(path) => audio::absolute_path(path)?,
        None => audio::default_workdir(&source.scenario.episode.episode_key)?,
    };
    let paths = RenderPaths::prepare(workdir, output)?;

    println!("Recording MP3: {}", paths.output.display());

    record_scenario(&loaded_config.values, &paths, &source.scenario).await?;

    println!("Done");

    Ok(())
}

pub async fn render(args: RenderArgs) -> Result<()> {
    let loaded_config = load_effective_config(args.config.as_deref(), args.config_overrides())?;

    let input = loaded_config
        .values
        .render_input
        .as_deref()
        .context("--input または [render].input を指定してください")?;
    let input = audio::absolute_path(input)?;
    let scenario = scenario::load(&input)?;
    scenario::validate(&scenario)?;

    ffmpeg::ensure_available()?;

    let output = loaded_config
        .values
        .render_output
        .as_deref()
        .context("--output または [render].output を指定してください")?;
    let output = audio::absolute_path(output)?;
    let workdir = match loaded_config.values.render_workdir.as_deref() {
        Some(path) => audio::absolute_path(path)?,
        None => audio::default_workdir(&scenario.episode.episode_key)?,
    };
    let paths = RenderPaths::prepare(workdir, output)?;

    info!(
        episode_key = %scenario.episode.episode_key,
        episode_title = %scenario.episode.title,
        sections = scenario.episode.scenario_json.sections.len(),
        "rendering episode"
    );

    record_scenario(&loaded_config.values, &paths, &scenario).await?;

    println!("MP3 を出力しました: {}", paths.output.display());

    Ok(())
}

struct RecordSourceData {
    scenario: ScenarioExport,
    raw_json: String,
}

async fn load_record_source(
    args: &RecordArgs,
    config: &ResolvedConfig,
) -> Result<RecordSourceData> {
    match args.source {
        RecordSource::Json => {
            if args.url.is_some() {
                bail!("--source json では --url を指定できません");
            }
            let input = args
                .input
                .as_deref()
                .context("--source json では --input を指定してください")?;
            let input = audio::absolute_path(input)?;
            println!("Source: json");
            let (scenario, raw_json) = scenario::load_source(&input)?;

            Ok(RecordSourceData { scenario, raw_json })
        }
        RecordSource::Upstream => {
            if args.input.is_some() {
                bail!("--source upstream では --input を指定できません");
            }
            let url = args
                .url
                .as_deref()
                .or(config.upstream_episode_url.as_deref())
                .context(
                    "--source upstream では --url または [upstream].episode_url を指定してください",
                )?;
            println!("Source: upstream");
            println!("Fetching episode JSON: {url}");

            let client = UpstreamClient::new(resolve_upstream_access_token(config));
            let raw_json = client.fetch_episode_json(url).await?;
            let scenario =
                scenario::parse(&raw_json).context("upstream API の JSON を解析できません")?;

            Ok(RecordSourceData { scenario, raw_json })
        }
    }
}

async fn record_scenario(
    config: &ResolvedConfig,
    paths: &RenderPaths,
    scenario: &ScenarioExport,
) -> Result<()> {
    let sections = scenario
        .episode
        .scenario_json
        .sections
        .iter()
        .map(RenderSection::from_section)
        .collect::<Vec<_>>();

    render_sections(config, paths, &sections).await
}

pub(crate) async fn render_scenario_to_mp3(
    config: &ResolvedConfig,
    scenario: &ScenarioExport,
    output: PathBuf,
    workdir: PathBuf,
) -> Result<()> {
    let paths = RenderPaths::prepare(workdir, output)?;
    record_scenario(config, &paths, scenario).await
}

fn resolve_upstream_access_token(config: &ResolvedConfig) -> Option<String> {
    std::env::var("VOICEPIPE_UPSTREAM_ACCESS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| config.upstream_access_token.clone())
}

fn default_record_output(config: &ResolvedConfig, episode_key: &str) -> Result<PathBuf> {
    let filename = format!("{}.mp3", audio::safe_file_component(episode_key, "episode"));
    let output = config.storage_audio_dir.join(filename);
    audio::absolute_path(&output)
}

fn write_json_output(path: &Path, json: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "JSON 出力ディレクトリを作成できません: {}",
                parent.display()
            )
        })?;
    }

    fs::write(path, json)
        .with_context(|| format!("Episode JSON を保存できません: {}", path.display()))
}

pub async fn preview(args: PreviewArgs) -> Result<()> {
    if args.max_sections == 0 {
        bail!("--max-sections は 1 以上を指定してください");
    }
    if args.max_chars_per_section == 0 {
        bail!("--max-chars-per-section は 1 以上を指定してください");
    }

    let loaded_config = load_effective_config(args.config.as_deref(), args.config_overrides())?;

    ffmpeg::ensure_available()?;

    let sections = preview_sections_from_args(&args, &loaded_config.values)?;
    let output = match loaded_config.values.preview_output.as_deref() {
        Some(path) => audio::absolute_path(path)?,
        None => audio::absolute_path(&default_preview_output(&loaded_config.values))?,
    };
    let workdir = match loaded_config.values.preview_workdir.as_deref() {
        Some(path) => audio::absolute_path(path)?,
        None => audio::absolute_path(&PathBuf::from("work/preview"))?,
    };
    let paths = RenderPaths::prepare(workdir, output)?;

    print_preview_summary(&sections, &loaded_config.values, &paths.output);
    render_sections(&loaded_config.values, &paths, &sections).await?;

    println!("Preview MP3 を出力しました: {}", paths.output.display());

    Ok(())
}

fn preview_sections_from_args(
    args: &PreviewArgs,
    config: &ResolvedConfig,
) -> Result<Vec<RenderSection>> {
    let mode_count = usize::from(args.input.is_some())
        + usize::from(args.text.is_some())
        + usize::from(args.stdin);

    if mode_count > 1 {
        bail!("preview には --input、--text、--stdin のいずれか 1 つだけを指定してください");
    }

    if let Some(input) = &args.input {
        return preview_sections_from_episode_path(
            input,
            args.max_sections,
            args.max_chars_per_section,
        );
    }

    if args.text.is_none()
        && !args.stdin
        && let Some(input) = &config.preview_input
    {
        return preview_sections_from_episode_path(
            input,
            args.max_sections,
            args.max_chars_per_section,
        );
    }

    if mode_count == 0 {
        bail!("preview には --input、--text、--stdin のいずれか 1 つだけを指定してください");
    }

    let text = if let Some(text) = &args.text {
        text.to_string()
    } else {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("標準入力を読み込めません")?;
        text
    };

    let trimmed = trim_preview_text(&text, args.max_chars_per_section);
    if trimmed.trim().is_empty() {
        bail!("preview text は空にできません");
    }

    Ok(vec![RenderSection {
        section_type: "preview_text".to_string(),
        title: "Inline Preview".to_string(),
        text: trimmed,
        estimated_duration_seconds: None,
    }])
}

fn preview_sections_from_episode_path(
    input: &std::path::Path,
    max_sections: usize,
    max_chars_per_section: usize,
) -> Result<Vec<RenderSection>> {
    let input = audio::absolute_path(input)?;
    let scenario = scenario::load(&input)?;
    scenario::validate(&scenario)?;

    select_preview_sections(
        &scenario.episode.scenario_json.sections,
        max_sections,
        max_chars_per_section,
    )
}

#[derive(Debug, Clone)]
struct RenderSection {
    section_type: String,
    title: String,
    text: String,
    estimated_duration_seconds: Option<u64>,
}

impl RenderSection {
    fn from_section(section: &Section) -> Self {
        Self {
            section_type: section.section_type.clone(),
            title: section.title.clone(),
            text: section.text.trim().to_string(),
            estimated_duration_seconds: section.estimated_duration_seconds,
        }
    }
}

fn load_effective_config(
    path: Option<&std::path::Path>,
    overrides: ConfigOverrides,
) -> Result<config::LoadedConfig> {
    let mut loaded_config = config::load(path)?;
    loaded_config.values.apply_overrides(overrides);
    loaded_config.values.validate()?;

    Ok(loaded_config)
}

async fn render_sections(
    config: &ResolvedConfig,
    paths: &RenderPaths,
    sections: &[RenderSection],
) -> Result<()> {
    let voicevox = VoicevoxClient::new(
        config.voicevox_endpoint.clone(),
        config.speaker,
        config.voice.clone(),
    );
    voicevox.ensure_ready().await?;

    let mut segment_paths = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        info!(
            section_index = index,
            section_type = %section.section_type,
            section_title = %section.title,
            estimated_duration_seconds = section.estimated_duration_seconds,
            "synthesizing section"
        );

        let wav = voicevox.synthesize(section.text.trim()).await?;
        let segment_path = paths.segment_path(index, &section.section_type);
        fs::write(&segment_path, wav)
            .with_context(|| format!("section WAV を保存できません: {}", segment_path.display()))?;
        segment_paths.push(segment_path);
    }

    if segment_paths.len() > 1 {
        ffmpeg::create_silence(&paths.silence_wav, DEFAULT_PAUSE_BETWEEN_SECTIONS_MS)?;
    }

    write_concat_file(paths, &segment_paths)?;
    ffmpeg::concatenate_wav(&paths.workdir)?;
    ffmpeg::encode_mp3(&paths.combined_wav, &paths.output, &config.bitrate)
}

fn select_preview_sections(
    sections: &[Section],
    max_sections: usize,
    max_chars_per_section: usize,
) -> Result<Vec<RenderSection>> {
    let mut selected = Vec::new();

    for section_type in ["opening", "topic", "closing"] {
        if selected.len() >= max_sections {
            break;
        }

        if let Some(section) = sections
            .iter()
            .find(|section| section.section_type == section_type)
        {
            let mut preview_section = RenderSection::from_section(section);
            preview_section.text = trim_preview_text(&preview_section.text, max_chars_per_section);
            selected.push(preview_section);
        }
    }

    if selected.is_empty() {
        bail!("preview 対象の section が見つかりません");
    }

    Ok(selected)
}

fn trim_preview_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let limited = trimmed.chars().take(max_chars).collect::<String>();
    let boundary = limited
        .char_indices()
        .filter(|(_, character)| matches!(character, '。' | '！' | '？'))
        .map(|(index, character)| index + character.len_utf8())
        .next_back();

    match boundary {
        Some(index) => limited[..index].to_string(),
        None => limited,
    }
}

fn default_preview_output(config: &ResolvedConfig) -> PathBuf {
    config.storage_preview_dir.join(format!(
        "preview_speaker{}_speed{}_pitch{}_intonation{}_pause{}.mp3",
        config.speaker,
        format_setting(config.voice.speed_scale),
        format_setting(config.voice.pitch_scale),
        format_setting(config.voice.intonation_scale),
        format_setting(config.voice.pause_length_scale),
    ))
}

fn format_setting(value: f64) -> String {
    let scaled = (value * 100.0).round() as i64;
    if scaled < 0 {
        format!("m{:03}", scaled.abs())
    } else {
        format!("{scaled:03}")
    }
}

fn print_preview_summary(
    sections: &[RenderSection],
    config: &ResolvedConfig,
    output: &std::path::Path,
) {
    println!("Preview sections:");
    for section in sections {
        println!("- {}: {}", section.section_type, section.title);
    }
    println!();
    println!("Voice settings:");
    println!("speaker={}", config.speaker);
    println!("speedScale={}", config.voice.speed_scale);
    println!("pitchScale={}", config.voice.pitch_scale);
    println!("intonationScale={}", config.voice.intonation_scale);
    println!("pauseLengthScale={}", config.voice.pause_length_scale);
    println!("volumeScale={}", config.voice.volume_scale);
    println!();
    println!("Output:");
    println!("{}", output.display());
}

fn write_concat_file(paths: &RenderPaths, segment_paths: &[PathBuf]) -> Result<()> {
    let mut content = String::from("ffconcat version 1.0\n");

    for (index, segment_path) in segment_paths.iter().enumerate() {
        content.push_str(&format!(
            "file '{}'\n",
            relative_to_workdir(paths, segment_path).display()
        ));

        if index + 1 < segment_paths.len() {
            content.push_str(&format!(
                "file '{}'\n",
                relative_to_workdir(paths, &paths.silence_wav).display()
            ));
        }
    }

    fs::write(&paths.concat_file, content).with_context(|| {
        format!(
            "ffconcat ファイルを保存できません: {}",
            paths.concat_file.display()
        )
    })
}

fn relative_to_workdir<'a>(
    paths: &'a RenderPaths,
    path: &'a std::path::Path,
) -> &'a std::path::Path {
    path.strip_prefix(&paths.workdir).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_preview_text_prefers_japanese_sentence_boundary() {
        assert_eq!(
            trim_preview_text("一文目です。二文目です。三文目です。", 10),
            "一文目です。"
        );
    }

    #[test]
    fn trim_preview_text_falls_back_to_character_count() {
        assert_eq!(trim_preview_text("abcdef", 3), "abc");
    }

    #[test]
    fn format_setting_uses_hundred_scaled_padded_values() {
        assert_eq!(format_setting(1.2), "120");
        assert_eq!(format_setting(0.05), "005");
        assert_eq!(format_setting(-0.05), "m005");
    }

    #[test]
    fn default_record_output_uses_storage_audio_dir_and_episode_key() {
        let config = ResolvedConfig {
            storage_audio_dir: PathBuf::from("custom/dist/record"),
            ..ResolvedConfig::default()
        };

        let output = default_record_output(&config, "episode-001").expect("path should resolve");

        assert!(output.ends_with("custom/dist/record/episode-001.mp3"));
    }

    #[test]
    fn default_preview_output_uses_storage_preview_dir() {
        let config = ResolvedConfig {
            storage_preview_dir: PathBuf::from("custom/dist/preview"),
            ..ResolvedConfig::default()
        };

        assert!(default_preview_output(&config).ends_with(
            "custom/dist/preview/preview_speaker3_speed120_pitch000_intonation090_pause130.mp3"
        ));
    }

    #[tokio::test]
    async fn record_json_source_rejects_url() {
        let args = record_args(RecordSource::Json);
        let args = RecordArgs {
            url: Some("https://example.com/api/episodes/latest".to_string()),
            ..args
        };

        let error = record_source_error(&args, &ResolvedConfig::default()).await;

        assert!(error.to_string().contains("--source json"));
    }

    #[tokio::test]
    async fn record_upstream_source_rejects_input() {
        let args = RecordArgs {
            input: Some(PathBuf::from("episode.json")),
            url: Some("https://example.com/api/episodes/latest".to_string()),
            ..record_args(RecordSource::Upstream)
        };

        let error = record_source_error(&args, &ResolvedConfig::default()).await;

        assert!(error.to_string().contains("--source upstream"));
    }

    #[tokio::test]
    async fn record_upstream_source_requires_url_or_config() {
        let args = record_args(RecordSource::Upstream);

        let error = record_source_error(&args, &ResolvedConfig::default()).await;

        assert!(error.to_string().contains("[upstream].episode_url"));
    }

    #[test]
    fn preview_text_requires_exactly_one_input_mode() {
        let args = PreviewArgs {
            config: None,
            input: None,
            text: None,
            stdin: false,
            output: None,
            workdir: Some(PathBuf::from("work/preview")),
            max_sections: 3,
            max_chars_per_section: 300,
            voicevox_endpoint: None,
            speaker: None,
            speed_scale: None,
            pitch_scale: None,
            intonation_scale: None,
            pause_length_scale: None,
            volume_scale: None,
        };

        assert!(preview_sections_from_args(&args, &ResolvedConfig::default()).is_err());
    }

    fn record_args(source: RecordSource) -> RecordArgs {
        RecordArgs {
            config: None,
            source,
            input: None,
            url: None,
            output: None,
            output_json: None,
            workdir: None,
            voicevox_endpoint: None,
            speaker: None,
            speed_scale: None,
            pitch_scale: None,
            intonation_scale: None,
            pause_length_scale: None,
            volume_scale: None,
        }
    }

    async fn record_source_error(args: &RecordArgs, config: &ResolvedConfig) -> anyhow::Error {
        match load_record_source(args, config).await {
            Ok(_) => panic!("record source should fail"),
            Err(error) => error,
        }
    }
}
