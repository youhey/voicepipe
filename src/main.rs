mod audio;
mod cli;
mod config;
mod doctor;
mod ffmpeg;
mod renderer;
mod scenario;
mod upstream;
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
        Commands::Record(args) => renderer::record(args).await,
        Commands::Render(args) => renderer::render(args).await,
        Commands::Preview(args) => renderer::preview(args).await,
        Commands::Speakers(args) => voicevox::print_speakers(args).await,
        Commands::Doctor(args) => doctor::run(args).await,
    }
}
