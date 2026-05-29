use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::voicevox::{
    DEFAULT_INTONATION_SCALE, DEFAULT_PAUSE_LENGTH_SCALE, DEFAULT_PITCH_SCALE, DEFAULT_SPEAKER,
    DEFAULT_SPEED_SCALE, DEFAULT_VOICEVOX_ENDPOINT, DEFAULT_VOLUME_SCALE,
};

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
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Input episode JSON exported from radiopipe.
    #[arg(long)]
    pub input: PathBuf,

    /// Output MP3 path.
    #[arg(long)]
    pub output: PathBuf,

    /// Working directory for section WAV files and ffmpeg intermediates.
    #[arg(long)]
    pub workdir: Option<PathBuf>,

    /// Local VOICEVOX Engine endpoint.
    #[arg(long, default_value = DEFAULT_VOICEVOX_ENDPOINT)]
    pub voicevox_endpoint: String,

    /// VOICEVOX speaker id.
    #[arg(long, default_value_t = DEFAULT_SPEAKER)]
    pub speaker: u32,

    /// VOICEVOX speedScale.
    #[arg(long, default_value_t = DEFAULT_SPEED_SCALE)]
    pub speed_scale: f64,

    /// VOICEVOX pitchScale.
    #[arg(long, default_value_t = DEFAULT_PITCH_SCALE)]
    pub pitch_scale: f64,

    /// VOICEVOX intonationScale.
    #[arg(long, default_value_t = DEFAULT_INTONATION_SCALE)]
    pub intonation_scale: f64,

    /// VOICEVOX pauseLengthScale.
    #[arg(long, default_value_t = DEFAULT_PAUSE_LENGTH_SCALE)]
    pub pause_length_scale: f64,

    /// VOICEVOX volumeScale.
    #[arg(long, default_value_t = DEFAULT_VOLUME_SCALE)]
    pub volume_scale: f64,
}
