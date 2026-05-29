use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail};

use crate::{audio, cli::DoctorArgs, config, ffmpeg, voicevox::VoicevoxClient};

pub async fn run(args: DoctorArgs) -> Result<()> {
    let mut failures = Vec::new();

    let loaded_config = match load_config(&args) {
        Ok(loaded) => {
            match &loaded.path {
                Some(path) => println!("ok: configuration file loaded: {}", path.display()),
                None => println!("ok: configuration file omitted; using built-in defaults"),
            }
            Some(loaded)
        }
        Err(error) => {
            println!("error: configuration file invalid: {error:#}");
            failures.push("configuration file");
            None
        }
    };

    match ffmpeg::ensure_available() {
        Ok(()) => println!("ok: ffmpeg available"),
        Err(error) => {
            println!("error: ffmpeg unavailable: {error:#}");
            failures.push("ffmpeg");
        }
    }

    if let Some(loaded) = loaded_config {
        let client = VoicevoxClient::new(
            loaded.values.voicevox_endpoint,
            loaded.values.speaker,
            loaded.values.voice,
        );
        match client.ensure_ready().await {
            Ok(()) => println!("ok: VOICEVOX Engine reachable"),
            Err(error) => {
                println!("error: VOICEVOX Engine unreachable: {error:#}");
                failures.push("voicevox");
            }
        }
    }

    for directory in [
        ("output directory", args.output_dir),
        ("work directory", args.workdir),
    ] {
        match ensure_directory_writable(&directory.1) {
            Ok(()) => println!("ok: {} writable: {}", directory.0, directory.1.display()),
            Err(error) => {
                println!(
                    "error: {} not writable: {}: {error:#}",
                    directory.0,
                    directory.1.display()
                );
                failures.push(directory.0);
            }
        }
    }

    if failures.is_empty() {
        println!("doctor: all checks passed");
        return Ok(());
    }

    bail!("doctor: {} check(s) failed", failures.len())
}

fn load_config(args: &DoctorArgs) -> Result<config::LoadedConfig> {
    let mut loaded = config::load(args.config.as_deref())?;
    loaded.values.apply_overrides(args.config_overrides());
    loaded.values.validate()?;

    Ok(loaded)
}

fn ensure_directory_writable(path: &Path) -> Result<()> {
    let absolute = audio::absolute_path(path)?;
    fs::create_dir_all(&absolute)
        .with_context(|| format!("ディレクトリを作成できません: {}", absolute.display()))?;

    let test_file = writable_test_file(&absolute);
    fs::write(&test_file, b"voicepipe doctor\n")
        .with_context(|| format!("テストファイルを書き込めません: {}", test_file.display()))?;
    fs::remove_file(&test_file)
        .with_context(|| format!("テストファイルを削除できません: {}", test_file.display()))?;

    Ok(())
}

fn writable_test_file(directory: &Path) -> PathBuf {
    directory.join(format!(".voicepipe-doctor-{}.tmp", process::id()))
}
