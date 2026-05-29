use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use tracing::info;

use crate::{
    audio::{self, DEFAULT_PAUSE_BETWEEN_SECTIONS_MS, RenderPaths},
    cli::RenderArgs,
    config, ffmpeg, scenario,
    voicevox::VoicevoxClient,
};

pub async fn render(args: RenderArgs) -> Result<()> {
    let input = audio::absolute_path(&args.input)?;
    let scenario = scenario::load(&input)?;
    scenario::validate(&scenario)?;

    let mut loaded_config = config::load(args.config.as_deref())?;
    loaded_config
        .values
        .apply_overrides(args.config_overrides());
    loaded_config.values.validate()?;

    ffmpeg::ensure_available()?;

    let output = audio::absolute_path(&args.output)?;
    let workdir = match args.workdir {
        Some(path) => audio::absolute_path(&path)?,
        None => audio::default_workdir(&scenario.episode.episode_key)?,
    };
    let paths = RenderPaths::prepare(workdir, output)?;

    let voicevox = VoicevoxClient::new(
        loaded_config.values.voicevox_endpoint.clone(),
        loaded_config.values.speaker,
        loaded_config.values.voice.clone(),
    );
    voicevox.ensure_ready().await?;

    info!(
        episode_key = %scenario.episode.episode_key,
        episode_title = %scenario.episode.title,
        sections = scenario.episode.scenario_json.sections.len(),
        "rendering episode"
    );

    let mut segment_paths = Vec::new();
    for (index, section) in scenario.episode.scenario_json.sections.iter().enumerate() {
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

    write_concat_file(&paths, &segment_paths)?;
    ffmpeg::concatenate_wav(&paths.workdir)?;
    ffmpeg::encode_mp3(
        &paths.combined_wav,
        &paths.output,
        &loaded_config.values.bitrate,
    )?;

    println!("MP3 を出力しました: {}", paths.output.display());

    Ok(())
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
