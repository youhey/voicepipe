mod audio;
mod cli;
mod config;
mod doctor;
mod ffmpeg;
mod renderer;
mod scenario;
mod voicevox;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Cli::parse().command {
        Commands::Render(args) => renderer::render(args).await,
        Commands::Speakers(args) => voicevox::print_speakers(args).await,
        Commands::Doctor(args) => doctor::run(args).await,
    }
}
