use std::{
    path::Path,
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};

pub fn ensure_available() -> Result<()> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map_err(|error| {
            anyhow!(
                "ffmpeg が見つかりません。PATH に ffmpeg を追加してから再実行してください: {}",
                error
            )
        })?;

    ensure_success(output, "ffmpeg の確認")
}

pub fn create_silence(path: &Path, pause_ms: u64) -> Result<()> {
    let duration = format!("{:.3}", pause_ms as f64 / 1000.0);
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("anullsrc=channel_layout=mono:sample_rate=24000")
        .arg("-t")
        .arg(duration)
        .arg("-ac")
        .arg("1")
        .arg(path)
        .output()
        .with_context(|| format!("無音 WAV を作成できません: {}", path.display()))?;

    ensure_success(output, "ffmpeg による無音 WAV 生成")
}

pub fn concatenate_wav(workdir: &Path) -> Result<()> {
    let output = Command::new("ffmpeg")
        .current_dir(workdir)
        .arg("-hide_banner")
        .arg("-y")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg("concat.ffconcat")
        .arg("-c")
        .arg("copy")
        .arg("combined.wav")
        .output()
        .with_context(|| format!("WAV を結合できません: {}", workdir.display()))?;

    ensure_success(output, "ffmpeg による WAV 結合")
}

pub fn encode_mp3(input: &Path, output_path: &Path, bitrate: &str) -> Result<()> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-codec:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg(bitrate)
        .arg(output_path)
        .output()
        .with_context(|| {
            format!(
                "MP3 をエンコードできません: {} -> {}",
                input.display(),
                output_path.display()
            )
        })?;

    ensure_success(output, "ffmpeg による MP3 エンコード")
}

fn ensure_success(output: Output, description: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stderr
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    if summary.is_empty() {
        bail!("{} が失敗しました: {}", description, output.status);
    }

    bail!(
        "{} が失敗しました: {}\n{}",
        description,
        output.status,
        summary
    );
}
