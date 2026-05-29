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

pub const CONFIG_STACK: [&str; 3] = [
    "voicepipe.toml",
    "voicepipe.dist.toml",
    "voicepipe.override.toml",
];
pub const DEFAULT_AUDIO_FORMAT: &str = "mp3";

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub values: ResolvedConfig,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub render_input: Option<PathBuf>,
    pub render_output: Option<PathBuf>,
    pub render_workdir: Option<PathBuf>,
    pub preview_input: Option<PathBuf>,
    pub preview_output: Option<PathBuf>,
    pub preview_workdir: Option<PathBuf>,
    pub upstream_episode_url: Option<String>,
    pub upstream_access_token: Option<String>,
    pub storage_json_dir: PathBuf,
    pub storage_audio_dir: PathBuf,
    pub voicevox_endpoint: String,
    pub speaker: u32,
    pub voice: VoiceOptions,
    pub bitrate: String,
    pub format: String,
}

#[derive(Debug, Default)]
pub struct ConfigOverrides {
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
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
    render: Option<FilePathConfig>,
    preview: Option<FilePathConfig>,
    upstream: Option<FileUpstreamConfig>,
    storage: Option<FileStorageConfig>,
    voicevox: Option<FileVoicevoxConfig>,
    voice: Option<FileVoiceConfig>,
    audio: Option<FileAudioConfig>,
}

#[derive(Debug, Deserialize)]
struct FileUpstreamConfig {
    episode_url: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileStorageConfig {
    json_dir: Option<PathBuf>,
    audio_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct FilePathConfig {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    workdir: Option<PathBuf>,
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
            render_input: None,
            render_output: None,
            render_workdir: None,
            preview_input: None,
            preview_output: None,
            preview_workdir: None,
            upstream_episode_url: None,
            upstream_access_token: None,
            storage_json_dir: PathBuf::from("storage/json"),
            storage_audio_dir: PathBuf::from("storage/audio"),
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
        if let Some(value) = overrides.input {
            self.render_input = Some(value.clone());
            self.preview_input = Some(value);
        }
        if let Some(value) = overrides.output {
            self.render_output = Some(value.clone());
            self.preview_output = Some(value);
        }
        if let Some(value) = overrides.workdir {
            self.render_workdir = Some(value.clone());
            self.preview_workdir = Some(value);
        }
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
        if let Some(url) = &self.upstream_episode_url
            && url.trim().is_empty()
        {
            bail!("upstream.episode_url は空にできません");
        }
        if let Some(token) = &self.upstream_access_token
            && token.trim().is_empty()
        {
            bail!("upstream.access_token は空にできません");
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

impl FilePathConfig {
    fn apply_to_render(self, resolved: &mut ResolvedConfig) {
        if let Some(value) = self.input {
            resolved.render_input = Some(value);
        }
        if let Some(value) = self.output {
            resolved.render_output = Some(value);
        }
        if let Some(value) = self.workdir {
            resolved.render_workdir = Some(value);
        }
    }

    fn apply_to_preview(self, resolved: &mut ResolvedConfig) {
        if let Some(value) = self.input {
            resolved.preview_input = Some(value);
        }
        if let Some(value) = self.output {
            resolved.preview_output = Some(value);
        }
        if let Some(value) = self.workdir {
            resolved.preview_workdir = Some(value);
        }
    }
}

impl FileConfig {
    fn apply_to(self, resolved: &mut ResolvedConfig) {
        if let Some(render) = self.render {
            render.apply_to_render(resolved);
        }

        if let Some(preview) = self.preview {
            preview.apply_to_preview(resolved);
        }

        if let Some(upstream) = self.upstream {
            if let Some(value) = upstream.episode_url {
                resolved.upstream_episode_url = Some(value);
            }
            if let Some(value) = upstream.access_token {
                resolved.upstream_access_token = Some(value);
            }
        }

        if let Some(storage) = self.storage {
            if let Some(value) = storage.json_dir {
                resolved.storage_json_dir = value;
            }
            if let Some(value) = storage.audio_dir {
                resolved.storage_audio_dir = value;
            }
        }

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
    }
}

pub fn load(path: Option<&Path>) -> Result<LoadedConfig> {
    load_with_base(path, Path::new("."))
}

fn load_with_base(path: Option<&Path>, base_dir: &Path) -> Result<LoadedConfig> {
    let paths = match path {
        Some(path) => vec![path.to_path_buf()],
        None => existing_config_stack(base_dir),
    };

    if paths.is_empty() {
        bail!(
            "設定ファイルが見つかりません: {} のいずれかを作成するか --config を指定してください",
            CONFIG_STACK.join(", ")
        );
    }

    let mut values = ResolvedConfig::default();
    for path in &paths {
        load_file(path)?.apply_to(&mut values);
    }
    values.validate()?;

    Ok(LoadedConfig { values, paths })
}

fn existing_config_stack(base_dir: &Path) -> Vec<PathBuf> {
    CONFIG_STACK
        .iter()
        .map(|name| base_dir.join(name))
        .filter(|path| path.exists())
        .collect()
}

fn load_file(path: &Path) -> Result<FileConfig> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("設定ファイルを読み込めません: {}", path.display()))?;
    toml::from_str::<FileConfig>(&source)
        .with_context(|| format!("設定ファイルの TOML を解析できません: {}", path.display()))
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
        let parsed = resolve_toml(
            r#"
            [render]
            input = "episode.json"
            output = "dist/episode.mp3"
            workdir = "work/episode"

            [voicevox]
            endpoint = "http://127.0.0.1:50022"
            speaker = 8

            [voice]
            speed_scale = 1.3

            [audio]
            bitrate = "128k"
            format = "mp3"

            [upstream]
            episode_url = "https://example.com/api/episodes/latest"
            access_token = "dummy-token"

            [storage]
            json_dir = "custom/json"
            audio_dir = "custom/audio"
            "#,
        );

        assert_eq!(parsed.render_input, Some(PathBuf::from("episode.json")));
        assert_eq!(
            parsed.render_output,
            Some(PathBuf::from("dist/episode.mp3"))
        );
        assert_eq!(parsed.render_workdir, Some(PathBuf::from("work/episode")));
        assert_eq!(parsed.voicevox_endpoint, "http://127.0.0.1:50022");
        assert_eq!(parsed.speaker, 8);
        assert_eq!(parsed.voice.speed_scale, 1.3);
        assert_eq!(parsed.voice.pitch_scale, DEFAULT_PITCH_SCALE);
        assert_eq!(parsed.bitrate, "128k");
        assert_eq!(
            parsed.upstream_episode_url,
            Some("https://example.com/api/episodes/latest".to_string())
        );
        assert_eq!(
            parsed.upstream_access_token,
            Some("dummy-token".to_string())
        );
        assert_eq!(parsed.storage_json_dir, PathBuf::from("custom/json"));
        assert_eq!(parsed.storage_audio_dir, PathBuf::from("custom/audio"));
    }

    #[test]
    fn applies_cli_overrides_over_config_values() {
        let mut config = ResolvedConfig {
            render_input: Some(PathBuf::from("config-input.json")),
            render_output: Some(PathBuf::from("config-output.mp3")),
            render_workdir: Some(PathBuf::from("config-work")),
            voicevox_endpoint: "http://127.0.0.1:50022".to_string(),
            speaker: 8,
            voice: VoiceOptions {
                speed_scale: 1.0,
                ..VoiceOptions::default()
            },
            bitrate: "128k".to_string(),
            format: "mp3".to_string(),
            ..ResolvedConfig::default()
        };

        config.apply_overrides(ConfigOverrides {
            input: Some(PathBuf::from("cli-input.json")),
            output: Some(PathBuf::from("cli-output.mp3")),
            workdir: Some(PathBuf::from("cli-work")),
            speaker: Some(3),
            speed_scale: Some(1.4),
            ..ConfigOverrides::default()
        });

        assert_eq!(config.render_input, Some(PathBuf::from("cli-input.json")));
        assert_eq!(config.render_output, Some(PathBuf::from("cli-output.mp3")));
        assert_eq!(config.render_workdir, Some(PathBuf::from("cli-work")));
        assert_eq!(config.voicevox_endpoint, "http://127.0.0.1:50022");
        assert_eq!(config.speaker, 3);
        assert_eq!(config.voice.speed_scale, 1.4);
        assert_eq!(config.bitrate, "128k");
    }

    #[test]
    fn loads_config_stack_in_declared_order() {
        let temp_dir = unique_temp_dir("voicepipe-config-stack");
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        fs::write(
            temp_dir.join("voicepipe.toml"),
            r#"
            [voicevox]
            speaker = 2

            [voice]
            speed_scale = 1.1
            "#,
        )
        .expect("base config should be written");
        fs::write(
            temp_dir.join("voicepipe.dist.toml"),
            r#"
            [voicevox]
            speaker = 3
            "#,
        )
        .expect("dist config should be written");
        fs::write(
            temp_dir.join("voicepipe.override.toml"),
            r#"
            [voice]
            speed_scale = 1.4
            "#,
        )
        .expect("override config should be written");

        let loaded = load_with_base(None, &temp_dir).expect("stack should load");

        assert_eq!(
            loaded
                .paths
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
                .collect::<Vec<_>>(),
            vec![
                "voicepipe.toml",
                "voicepipe.dist.toml",
                "voicepipe.override.toml"
            ]
        );
        assert_eq!(loaded.values.speaker, 3);
        assert_eq!(loaded.values.voice.speed_scale, 1.4);

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[test]
    fn explicit_config_ignores_stack_files() {
        let temp_dir = unique_temp_dir("voicepipe-explicit-config");
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        fs::write(
            temp_dir.join("voicepipe.toml"),
            r#"
            [voicevox]
            speaker = 2
            "#,
        )
        .expect("base config should be written");
        let explicit = temp_dir.join("custom.toml");
        fs::write(
            &explicit,
            r#"
            [voicevox]
            speaker = 8
            "#,
        )
        .expect("explicit config should be written");

        let loaded =
            load_with_base(Some(&explicit), &temp_dir).expect("explicit config should load");

        assert_eq!(loaded.paths, vec![explicit]);
        assert_eq!(loaded.values.speaker, 8);

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[test]
    fn missing_default_config_stack_is_an_error() {
        let temp_dir = unique_temp_dir("voicepipe-missing-config");
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");

        let error = load_with_base(None, &temp_dir).expect_err("missing stack should fail");

        assert!(error.to_string().contains("設定ファイルが見つかりません"));

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    fn resolve_toml(source: &str) -> ResolvedConfig {
        let mut resolved = ResolvedConfig::default();
        toml::from_str::<FileConfig>(source)
            .expect("config should parse")
            .apply_to(&mut resolved);
        resolved
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
    }
}
