use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{
    audio::DEFAULT_OUTPUT_BITRATE,
    voicevox::{DEFAULT_SPEAKER, DEFAULT_VOICEVOX_ENDPOINT, VoiceOptions},
};

pub const DEFAULT_CONFIG_PATH: &str = "voicepipe.toml";
pub const DEFAULT_AUDIO_FORMAT: &str = "mp3";

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub values: ResolvedConfig,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub voicevox_endpoint: String,
    pub speaker: u32,
    pub voice: VoiceOptions,
    pub bitrate: String,
    pub format: String,
}

#[derive(Debug, Default)]
pub struct ConfigOverrides {
    pub voicevox_endpoint: Option<String>,
    pub speaker: Option<u32>,
    pub speed_scale: Option<f64>,
    pub pitch_scale: Option<f64>,
    pub intonation_scale: Option<f64>,
    pub pause_length_scale: Option<f64>,
    pub volume_scale: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    voicevox: Option<FileVoicevoxConfig>,
    voice: Option<FileVoiceConfig>,
    audio: Option<FileAudioConfig>,
}

#[derive(Debug, Deserialize)]
struct FileVoicevoxConfig {
    endpoint: Option<String>,
    speaker: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FileVoiceConfig {
    speed_scale: Option<f64>,
    pitch_scale: Option<f64>,
    intonation_scale: Option<f64>,
    pause_length_scale: Option<f64>,
    volume_scale: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FileAudioConfig {
    bitrate: Option<String>,
    format: Option<String>,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        Self {
            voicevox_endpoint: DEFAULT_VOICEVOX_ENDPOINT.to_string(),
            speaker: DEFAULT_SPEAKER,
            voice: VoiceOptions::default(),
            bitrate: DEFAULT_OUTPUT_BITRATE.to_string(),
            format: DEFAULT_AUDIO_FORMAT.to_string(),
        }
    }
}

impl ResolvedConfig {
    pub fn apply_overrides(&mut self, overrides: ConfigOverrides) {
        if let Some(value) = overrides.voicevox_endpoint {
            self.voicevox_endpoint = value;
        }
        if let Some(value) = overrides.speaker {
            self.speaker = value;
        }
        if let Some(value) = overrides.speed_scale {
            self.voice.speed_scale = value;
        }
        if let Some(value) = overrides.pitch_scale {
            self.voice.pitch_scale = value;
        }
        if let Some(value) = overrides.intonation_scale {
            self.voice.intonation_scale = value;
        }
        if let Some(value) = overrides.pause_length_scale {
            self.voice.pause_length_scale = value;
        }
        if let Some(value) = overrides.volume_scale {
            self.voice.volume_scale = value;
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.voicevox_endpoint.trim().is_empty() {
            bail!("voicevox.endpoint は空にできません");
        }
        if self.bitrate.trim().is_empty() {
            bail!("audio.bitrate は空にできません");
        }
        if self.format != DEFAULT_AUDIO_FORMAT {
            bail!("audio.format は mp3 のみ対応しています: {}", self.format);
        }

        validate_scale("voice.speed_scale", self.voice.speed_scale)?;
        validate_scale("voice.intonation_scale", self.voice.intonation_scale)?;
        validate_scale("voice.pause_length_scale", self.voice.pause_length_scale)?;
        validate_scale("voice.volume_scale", self.voice.volume_scale)?;

        Ok(())
    }
}

impl FileConfig {
    fn into_resolved(self) -> ResolvedConfig {
        let mut resolved = ResolvedConfig::default();

        if let Some(voicevox) = self.voicevox {
            if let Some(value) = voicevox.endpoint {
                resolved.voicevox_endpoint = value;
            }
            if let Some(value) = voicevox.speaker {
                resolved.speaker = value;
            }
        }

        if let Some(voice) = self.voice {
            if let Some(value) = voice.speed_scale {
                resolved.voice.speed_scale = value;
            }
            if let Some(value) = voice.pitch_scale {
                resolved.voice.pitch_scale = value;
            }
            if let Some(value) = voice.intonation_scale {
                resolved.voice.intonation_scale = value;
            }
            if let Some(value) = voice.pause_length_scale {
                resolved.voice.pause_length_scale = value;
            }
            if let Some(value) = voice.volume_scale {
                resolved.voice.volume_scale = value;
            }
        }

        if let Some(audio) = self.audio {
            if let Some(value) = audio.bitrate {
                resolved.bitrate = value;
            }
            if let Some(value) = audio.format {
                resolved.format = value;
            }
        }

        resolved
    }
}

pub fn load(path: Option<&Path>) -> Result<LoadedConfig> {
    let selected_path = match path {
        Some(path) => Some(path.to_path_buf()),
        None => {
            let default_path = PathBuf::from(DEFAULT_CONFIG_PATH);
            default_path.exists().then_some(default_path)
        }
    };

    let values = match selected_path.as_deref() {
        Some(path) => load_file(path)?,
        None => ResolvedConfig::default(),
    };
    values.validate()?;

    Ok(LoadedConfig {
        values,
        path: selected_path,
    })
}

fn load_file(path: &Path) -> Result<ResolvedConfig> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("設定ファイルを読み込めません: {}", path.display()))?;
    let file_config = toml::from_str::<FileConfig>(&source)
        .with_context(|| format!("設定ファイルの TOML を解析できません: {}", path.display()))?;

    Ok(file_config.into_resolved())
}

fn validate_scale(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        bail!("{} は 0 以上の有限数である必要があります: {}", name, value);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voicevox::DEFAULT_PITCH_SCALE;

    #[test]
    fn resolves_file_config_over_defaults() {
        let parsed = toml::from_str::<FileConfig>(
            r#"
            [voicevox]
            endpoint = "http://127.0.0.1:50022"
            speaker = 8

            [voice]
            speed_scale = 1.3

            [audio]
            bitrate = "128k"
            format = "mp3"
            "#,
        )
        .expect("config should parse")
        .into_resolved();

        assert_eq!(parsed.voicevox_endpoint, "http://127.0.0.1:50022");
        assert_eq!(parsed.speaker, 8);
        assert_eq!(parsed.voice.speed_scale, 1.3);
        assert_eq!(parsed.voice.pitch_scale, DEFAULT_PITCH_SCALE);
        assert_eq!(parsed.bitrate, "128k");
    }

    #[test]
    fn applies_cli_overrides_over_config_values() {
        let mut config = ResolvedConfig {
            voicevox_endpoint: "http://127.0.0.1:50022".to_string(),
            speaker: 8,
            voice: VoiceOptions {
                speed_scale: 1.0,
                ..VoiceOptions::default()
            },
            bitrate: "128k".to_string(),
            format: "mp3".to_string(),
        };

        config.apply_overrides(ConfigOverrides {
            speaker: Some(3),
            speed_scale: Some(1.4),
            ..ConfigOverrides::default()
        });

        assert_eq!(config.voicevox_endpoint, "http://127.0.0.1:50022");
        assert_eq!(config.speaker, 3);
        assert_eq!(config.voice.speed_scale, 1.4);
        assert_eq!(config.bitrate, "128k");
    }
}
