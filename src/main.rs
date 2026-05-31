mod audio;
mod cli;
mod config;
mod daemon;
mod doctor;
mod downstream;
mod ffmpeg;
mod ledger;
mod onair;
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
        Commands::Daemon(args) => daemon::run(args).await,
        Commands::Onair(args) => onair::run(args).await,
        Commands::Record(args) => renderer::record(args).await,
        Commands::Render(args) => renderer::render(args).await,
        Commands::Preview(args) => renderer::preview(args).await,
        Commands::Speakers(args) => voicevox::print_speakers(args).await,
        Commands::Doctor(args) => doctor::run(args).await,
    }
}
