use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::config::ConfigOverrides;

#[derive(Debug, Parser)]
#[command(
    name = "voicepipe",
    about = "Render radiopipe scenario JSON into narrated audio"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Render an episode JSON file into an MP3 file.
    Render(RenderArgs),

    /// Render a short MP3 preview for voice tuning.
    Preview(PreviewArgs),

    /// List available VOICEVOX speakers and styles.
    Speakers(VoicevoxArgs),

    /// Validate local rendering prerequisites.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Input episode JSON exported from radiopipe. Overrides [render].input.
    #[arg(long)]
    pub input: Option<PathBuf>,

    /// Output MP3 path. Overrides [render].output.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Working directory for section WAV files and ffmpeg intermediates.
    #[arg(long)]
    pub workdir: Option<PathBuf>,

    /// Local VOICEVOX Engine endpoint.
    #[arg(long)]
    pub voicevox_endpoint: Option<String>,

    /// VOICEVOX speaker id.
    #[arg(long)]
    pub speaker: Option<u32>,

    /// VOICEVOX speedScale.
    #[arg(long)]
    pub speed_scale: Option<f64>,

    /// VOICEVOX pitchScale.
    #[arg(long)]
    pub pitch_scale: Option<f64>,

    /// VOICEVOX intonationScale.
    #[arg(long)]
    pub intonation_scale: Option<f64>,

    /// VOICEVOX pauseLengthScale.
    #[arg(long)]
    pub pause_length_scale: Option<f64>,

    /// VOICEVOX volumeScale.
    #[arg(long)]
    pub volume_scale: Option<f64>,
}

#[derive(Debug, Args)]
pub struct PreviewArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Input episode JSON exported from radiopipe. Overrides [preview].input.
    #[arg(long)]
    pub input: Option<PathBuf>,

    /// Inline text to synthesize as a single-section preview.
    #[arg(long)]
    pub text: Option<String>,

    /// Read inline preview text from standard input.
    #[arg(long)]
    pub stdin: bool,

    /// Output preview MP3 path. Overrides [preview].output.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Working directory for preview WAV files and ffmpeg intermediates.
    #[arg(long)]
    pub workdir: Option<PathBuf>,

    /// Maximum number of preview sections.
    #[arg(long, default_value_t = 3)]
    pub max_sections: usize,

    /// Maximum text characters per preview section.
    #[arg(long, default_value_t = 300)]
    pub max_chars_per_section: usize,

    /// Local VOICEVOX Engine endpoint.
    #[arg(long)]
    pub voicevox_endpoint: Option<String>,

    /// VOICEVOX speaker id.
    #[arg(long)]
    pub speaker: Option<u32>,

    /// VOICEVOX speedScale.
    #[arg(long)]
    pub speed_scale: Option<f64>,

    /// VOICEVOX pitchScale.
    #[arg(long)]
    pub pitch_scale: Option<f64>,

    /// VOICEVOX intonationScale.
    #[arg(long)]
    pub intonation_scale: Option<f64>,

    /// VOICEVOX pauseLengthScale.
    #[arg(long)]
    pub pause_length_scale: Option<f64>,

    /// VOICEVOX volumeScale.
    #[arg(long)]
    pub volume_scale: Option<f64>,
}

#[derive(Debug, Args)]
pub struct VoicevoxArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Local VOICEVOX Engine endpoint.
    #[arg(long)]
    pub voicevox_endpoint: Option<String>,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Local VOICEVOX Engine endpoint.
    #[arg(long)]
    pub voicevox_endpoint: Option<String>,

    /// Output directory to check for writability.
    #[arg(long, default_value = "dist")]
    pub output_dir: PathBuf,

    /// Working directory to check for writability.
    #[arg(long, default_value = "work")]
    pub workdir: PathBuf,
}

impl RenderArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            input: self.input.clone(),
            output: self.output.clone(),
            workdir: self.workdir.clone(),
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            speaker: self.speaker,
            speed_scale: self.speed_scale,
            pitch_scale: self.pitch_scale,
            intonation_scale: self.intonation_scale,
            pause_length_scale: self.pause_length_scale,
            volume_scale: self.volume_scale,
        }
    }
}

impl PreviewArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            input: self.input.clone(),
            output: self.output.clone(),
            workdir: self.workdir.clone(),
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            speaker: self.speaker,
            speed_scale: self.speed_scale,
            pitch_scale: self.pitch_scale,
            intonation_scale: self.intonation_scale,
            pause_length_scale: self.pause_length_scale,
            volume_scale: self.volume_scale,
        }
    }
}

impl VoicevoxArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            ..ConfigOverrides::default()
        }
    }
}

impl DoctorArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            ..ConfigOverrides::default()
        }
    }
}
