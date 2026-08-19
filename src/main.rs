mod agent;
mod cli;
mod config;
mod context;
mod context_window;
mod cost;
mod gateway;
mod harness;
mod mcp;
mod memory;
mod providers;
mod sandbox;
mod session;
mod skill;
mod team;
mod tools;
mod transcript;

use clap::Parser;
use cli::CliArgs;

#[tokio::main]
async fn main() {
    // Logging.
    //
    // `warn` overall, with two dependency targets turned down to `error`.
    // Both are chatty about conditions the user cannot act on mid-turn:
    //
    // - `adk_tool::mcp` logs "failed to list tools from server, skipping
    //   server" **once per server, every time the toolset is listed** — which
    //   is every turn. One broken server becomes an unbounded stream of
    //   identical lines, and they print straight through the REPL spinner.
    //   The condition itself is not lost: per-server state is in `/status` and
    //   in the UI's Tools panel, which is where a broken server belongs.
    // - `rmcp` reports `JoinError::Cancelled` when an SSE task is cancelled at
    //   teardown. That is a shutdown, not a fault.
    //
    // Errors from both still print. `HARNESS_LOG` overrides all of it —
    // `HARNESS_LOG=warn` restores the old behaviour, and
    // `HARNESS_LOG=adk_tool::mcp=debug` goes the other way when an MCP server
    // is actually being debugged.
    const DEFAULT_LOG: &str = "warn,adk_tool::mcp=error,rmcp=error";
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HARNESS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG)),
        )
        .init();

    let args = CliArgs::parse();

    if let Err(e) = cli::run(args).await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
