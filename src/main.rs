mod cli;
mod config;
mod context;
mod harness;
mod mcp;
mod memory;
mod providers;
mod sandbox;
mod session;
mod skill;
mod tools;

use clap::Parser;
use cli::CliArgs;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HARNESS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let args = CliArgs::parse();

    if let Err(e) = cli::run(args).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
