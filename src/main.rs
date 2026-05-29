mod audio;
mod cli;
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
    }
}
