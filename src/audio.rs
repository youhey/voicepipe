use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub const DEFAULT_PAUSE_BETWEEN_SECTIONS_MS: u64 = 800;
pub const DEFAULT_OUTPUT_BITRATE: &str = "192k";

#[derive(Debug)]
pub struct RenderPaths {
    pub workdir: PathBuf,
    pub segments_dir: PathBuf,
    pub concat_file: PathBuf,
    pub combined_wav: PathBuf,
    pub silence_wav: PathBuf,
    pub output: PathBuf,
}

impl RenderPaths {
    pub fn prepare(workdir: PathBuf, output: PathBuf) -> Result<Self> {
        let segments_dir = workdir.join("segments");
        let silence_dir = workdir.join("silence");

        fs::create_dir_all(&segments_dir).with_context(|| {
            format!(
                "segments ディレクトリを作成できません: {}",
                segments_dir.display()
            )
        })?;
        fs::create_dir_all(&silence_dir).with_context(|| {
            format!(
                "silence ディレクトリを作成できません: {}",
                silence_dir.display()
            )
        })?;

        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!("出力ディレクトリを作成できません: {}", parent.display())
            })?;
        }

        Ok(Self {
            concat_file: workdir.join("concat.ffconcat"),
            combined_wav: workdir.join("combined.wav"),
            silence_wav: silence_dir.join(format!(
                "pause-{:04}ms.wav",
                DEFAULT_PAUSE_BETWEEN_SECTIONS_MS
            )),
            workdir,
            segments_dir,
            output,
        })
    }

    pub fn segment_path(&self, index: usize, section_type: &str) -> PathBuf {
        self.segments_dir.join(format!(
            "{:03}_{}.wav",
            index,
            safe_file_component(section_type, "section")
        ))
    }
}

pub fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()
            .context("現在のディレクトリを取得できません")?
            .join(path))
    }
}

pub fn default_workdir(episode_key: &str) -> Result<PathBuf> {
    Ok(env::current_dir()
        .context("現在のディレクトリを取得できません")?
        .join("work")
        .join("record")
        .join(safe_file_component(episode_key, "episode")))
}

pub fn safe_file_component(value: &str, fallback: &str) -> String {
    let mut result = String::new();
    let mut previous_dash = false;

    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            result.push(character);
            previous_dash = false;
        } else if !previous_dash {
            result.push('-');
            previous_dash = true;
        }
    }

    let trimmed = result.trim_matches('-');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_component_keeps_ascii_words() {
        assert_eq!(
            safe_file_component("Opening Talk", "section"),
            "opening-talk"
        );
    }

    #[test]
    fn safe_file_component_uses_fallback_for_non_ascii_only_values() {
        assert_eq!(safe_file_component("オープニング", "section"), "section");
    }
}
