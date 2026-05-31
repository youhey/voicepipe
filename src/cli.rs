use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::config::ConfigOverrides;

#[derive(Debug, Parser)]
#[command(
    name = "voicepipe",
    about = "Record radiopipe Episode JSON into narrated audio"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Periodically run the onair workflow.
    Daemon(DaemonArgs),

    /// Discover upstream episodes, record audio, upload downstream, and track state.
    Onair(OnAirArgs),

    /// Record an episode from a local JSON file or upstream API into an MP3 file.
    Record(RecordArgs),

    /// Legacy local JSON rendering command.
    Render(RenderArgs),

    /// Render a short MP3 preview for voice tuning.
    Preview(PreviewArgs),

    /// List available VOICEVOX speakers and styles.
    Speakers(VoicevoxArgs),

    /// Validate local rendering prerequisites.
    Doctor(DoctorArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RecordSource {
    Upstream,
    Json,
}

#[derive(Debug, Args)]
pub struct OnAirArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Maximum number of discovered episodes to process.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Discover unprocessed episodes without download, recording, upload, or ledger writes.
    #[arg(long)]
    pub dry_run: bool,

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

#[derive(Debug, Clone, Args)]
pub struct DaemonArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Seconds to wait between onair cycles.
    #[arg(long, default_value_t = 300)]
    pub interval: u64,

    /// Run one onair cycle and exit.
    #[arg(long)]
    pub once: bool,

    /// Maximum number of discovered episodes to process per cycle.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Discover episodes without download, recording, upload, or ledger writes.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Configuration file path. When omitted, the default config stack is used.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Episode JSON source.
    #[arg(long, value_enum)]
    pub source: RecordSource,

    /// Input episode JSON path for --source json.
    #[arg(long)]
    pub input: Option<PathBuf>,

    /// Upstream episode API URL for --source upstream.
    #[arg(long)]
    pub url: Option<String>,

    /// Output MP3 path.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Optional path to save the Episode JSON used for recording.
    #[arg(long)]
    pub output_json: Option<PathBuf>,

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

impl OnAirArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            speaker: self.speaker,
            speed_scale: self.speed_scale,
            pitch_scale: self.pitch_scale,
            intonation_scale: self.intonation_scale,
            pause_length_scale: self.pause_length_scale,
            volume_scale: self.volume_scale,
            ..ConfigOverrides::default()
        }
    }
}

impl DaemonArgs {
    pub fn to_onair_args(&self) -> OnAirArgs {
        OnAirArgs {
            config: self.config.clone(),
            limit: self.limit,
            dry_run: self.dry_run,
            voicevox_endpoint: None,
            speaker: None,
            speed_scale: None,
            pitch_scale: None,
            intonation_scale: None,
            pause_length_scale: None,
            volume_scale: None,
        }
    }
}

impl RecordArgs {
    pub fn config_overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            voicevox_endpoint: self.voicevox_endpoint.clone(),
            speaker: self.speaker,
            speed_scale: self.speed_scale,
            pitch_scale: self.pitch_scale,
            intonation_scale: self.intonation_scale,
            pause_length_scale: self.pause_length_scale,
            volume_scale: self.volume_scale,
            ..ConfigOverrides::default()
        }
    }
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
