mod banner;
mod commands;
mod oneshot;
mod repl;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "agent-harness", version, about = "Rust-native AI agent workspace")]
pub struct CliArgs {
    /// Run a single prompt and exit
    #[arg(short = 'p', long = "prompt")]
    pub prompt: Option<String>,

    /// Override model
    #[arg(long = "model")]
    pub model: Option<String>,

    /// Override provider
    #[arg(long = "provider")]
    pub provider: Option<String>,

    /// Set working directory
    #[arg(long = "project")]
    pub project: Option<String>,

    /// Set permission mode: strict (default), auto, yolo
    #[arg(long = "permission", default_value = "strict")]
    pub permission: String,

    /// Session ID to resume
    #[arg(long = "resume")]
    pub resume: Option<String>,
}

/// Main CLI entry point.
pub async fn run(args: CliArgs) -> anyhow::Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    let config = crate::config::HarnessConfig::from_cli_args(&args)?;
    let mut harness = crate::harness::Harness::build(config).await?;

    // Apply CLI overrides (rebuilds runner internally)
    if let Some(provider) = &args.provider {
        if let Some(model) = &args.model {
            harness.switch(provider, model)?;
        } else {
            harness.switch_provider(provider)?;
        }
    } else if let Some(model) = &args.model {
        harness.switch_model(model)?;
    }

    match &args.prompt {
        Some(prompt) => oneshot::run(&harness, prompt).await,
        None => repl::run(&mut harness).await,
    }
}
