use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
    #[arg(long, default_value = "http://127.0.0.1:50021")]
    pub voicevox_endpoint: String,

    /// VOICEVOX speaker id.
    #[arg(long, default_value_t = 3)]
    pub speaker: u32,
}
