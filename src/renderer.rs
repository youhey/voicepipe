use std::{fs, io::Read, path::PathBuf};

use anyhow::{Context, Result, bail};
use tracing::info;

use crate::{
    audio::{self, DEFAULT_PAUSE_BETWEEN_SECTIONS_MS, RenderPaths},
    cli::{PreviewArgs, RenderArgs},
    config::{self, ConfigOverrides, ResolvedConfig},
    ffmpeg, scenario,
    scenario::Section,
    voicevox::VoicevoxClient,
};

pub async fn render(args: RenderArgs) -> Result<()> {
    let input = audio::absolute_path(&args.input)?;
    let scenario = scenario::load(&input)?;
    scenario::validate(&scenario)?;

    let loaded_config = load_effective_config(args.config.as_deref(), args.config_overrides())?;

    ffmpeg::ensure_available()?;

    let output = audio::absolute_path(&args.output)?;
    let workdir = match args.workdir {
        Some(path) => audio::absolute_path(&path)?,
        None => audio::default_workdir(&scenario.episode.episode_key)?,
    };
    let paths = RenderPaths::prepare(workdir, output)?;

    info!(
        episode_key = %scenario.episode.episode_key,
        episode_title = %scenario.episode.title,
        sections = scenario.episode.scenario_json.sections.len(),
        "rendering episode"
    );

    let sections = scenario
        .episode
        .scenario_json
        .sections
        .iter()
        .map(RenderSection::from_section)
        .collect::<Vec<_>>();
    render_sections(&loaded_config.values, &paths, &sections).await?;

    println!("MP3 を出力しました: {}", paths.output.display());

    Ok(())
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

    let sections = preview_sections_from_args(&args)?;
    let output = match args.output {
        Some(path) => audio::absolute_path(&path)?,
        None => audio::absolute_path(&default_preview_output(&loaded_config.values))?,
    };
    let paths = RenderPaths::prepare(audio::absolute_path(&args.workdir)?, output)?;

    print_preview_summary(&sections, &loaded_config.values, &paths.output);
    render_sections(&loaded_config.values, &paths, &sections).await?;

    println!("Preview MP3 を出力しました: {}", paths.output.display());

    Ok(())
}

fn preview_sections_from_args(args: &PreviewArgs) -> Result<Vec<RenderSection>> {
    let mode_count = usize::from(args.input.is_some())
        + usize::from(args.text.is_some())
        + usize::from(args.stdin);

    if mode_count != 1 {
        bail!("preview には --input、--text、--stdin のいずれか 1 つだけを指定してください");
    }

    if let Some(input) = &args.input {
        let input = audio::absolute_path(input)?;
        let scenario = scenario::load(&input)?;
        scenario::validate(&scenario)?;

        return select_preview_sections(
            &scenario.episode.scenario_json.sections,
            args.max_sections,
            args.max_chars_per_section,
        );
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
    PathBuf::from("dist").join(format!(
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
    fn preview_text_requires_exactly_one_input_mode() {
        let args = PreviewArgs {
            config: None,
            input: None,
            text: None,
            stdin: false,
            output: None,
            workdir: PathBuf::from("work/preview"),
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

        assert!(preview_sections_from_args(&args).is_err());
    }
}
