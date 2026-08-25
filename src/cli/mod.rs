mod banner;
mod commands;
pub mod oneshot;
mod repl;
pub mod status;
pub mod team_cmd;
mod team_worker;

use std::sync::Arc;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "momo-fetch",
    version,
    about = "MOMO Fetch — AI coding companion",
    long_about = "MOMO Fetch — AI coding companion.\n\n\
        With no subcommand it starts the REPL (or runs one prompt with -p). \
        The `team` subcommand controls agent teams headlessly, printing JSON \
        on stdout so an agent can drive them through shell_exec."
)]
pub struct CliArgs {
    /// Headless subcommand. Omit it for the REPL / one-shot behaviour.
    #[command(subcommand)]
    pub command: Option<Command>,

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

    /// Run mode: "repl" (default) or "memory-sidecar" (Option C: separate process sidecar)
    #[arg(long = "mode", default_value = "repl")]
    pub mode: String,

    /// Start as a specific agent specialist (from .harness/agents/<name>.md)
    #[arg(short = 'a', long = "agent")]
    pub agent: Option<String>,

    /// Test MCP server connections and exit
    #[arg(long = "test-mcp")]
    pub test_mcp: bool,

    /// Run as a team worker under this name: do the `-p` task, then stay up
    /// polling the mailbox for more (see `momo-fetch team start`).
    ///
    /// Without it, `-p` is a one-shot: the task runs and the process exits.
    #[arg(long = "team-worker", value_name = "NAME")]
    pub team_worker: Option<String>,

    /// Mailbox directory, overriding `<project>/.harness/mailbox`.
    ///
    /// A worker in a git worktree has its own `.harness/`, so without this it
    /// would talk into a mailbox the lead never reads.
    #[arg(long = "mailbox", value_name = "DIR")]
    pub mailbox: Option<String>,

    /// Start the API gateway server
    #[arg(long = "gateway")]
    pub gateway: bool,

    /// Gateway port, overriding .harness/gateway.json. Use 0 to let the OS pick
    /// a free port — the chosen one is printed as `MOMO_GATEWAY_LISTENING <url>`.
    #[arg(long = "gateway-port")]
    pub gateway_port: Option<u16>,

    /// Gateway bind address (default: 127.0.0.1). Binding a non-loopback
    /// address requires auth to be enabled in .harness/gateway.json.
    #[arg(long = "gateway-bind")]
    pub gateway_bind: Option<std::net::IpAddr>,

    /// Additionally allow this CORS origin (repeatable).
    ///
    /// For the desktop shell, whose webview origin (`tauri://localhost`) is
    /// cross-origin to the loopback gateway. Applies to this run only and is
    /// never written to `.harness/gateway.json`.
    #[arg(long = "gateway-allow-origin", value_name = "ORIGIN")]
    pub gateway_allow_origin: Vec<String>,
}

/// Subcommands. Every flat flag above keeps working with no subcommand given,
/// so `momo-fetch`, `momo-fetch -p …`, `--gateway` and friends are untouched.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Control an agent team without entering the REPL (JSON on stdout)
    ///
    /// Exit codes: 0 success, 1 error, 2 state conflict.
    Team {
        #[command(subcommand)]
        action: team_cmd::TeamAction,
    },
}

/// Main CLI entry point.
pub async fn run(args: CliArgs) -> anyhow::Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Headless subcommands run before the harness is built: they touch only
    // `.harness/` on disk, and they must not need a provider or an API key.
    if let Some(Command::Team { action }) = &args.command {
        let code = team_cmd::run(action, args.project.as_deref());
        // stdout is the contract here — flush before the exit skips Drop.
        use std::io::Write;
        let _ = std::io::stdout().flush();
        std::process::exit(code);
    }

    // Check for memory-sidecar mode (Option C)
    if args.mode == "memory-sidecar" {
        return run_memory_sidecar(&args).await;
    }

    let config = crate::config::HarnessConfig::from_cli_args(&args)?;

    // Gateway mode: start HTTP API server
    if args.gateway {
        let overrides = crate::gateway::BindOverrides {
            port: args.gateway_port,
            bind: args.gateway_bind,
            allow_origins: args.gateway_allow_origin.clone(),
        };
        return crate::gateway::run(config, overrides).await;
    }
    let mut harness = crate::harness::Harness::build(config).await?;

    // --test-mcp: connect all MCP servers, report status, exit
    if args.test_mcp {
        return run_test_mcp(&harness).await;
    }

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

    // A standby team worker: same one-shot turn first, then it stays up
    // polling its inbox instead of exiting.
    if let Some(worker_name) = &args.team_worker {
        let result = team_worker::run(&harness, worker_name, args.prompt.as_deref()).await;
        let _ = harness.mcp_service().shutdown().await;
        return result;
    }

    match &args.prompt {
        Some(prompt) => {
            let result = oneshot::run(&harness, prompt).await;
            let _ = harness.mcp_service().shutdown().await;
            result
        }
        None => {
            let result = repl::run(&mut harness).await;
            let _ = harness.mcp_service().shutdown().await;
            result
        }
    }
}

/// Test all MCP server connections and report results.
async fn run_test_mcp(harness: &crate::harness::Harness) -> anyhow::Result<()> {
    use colored::Colorize;

    struct TestCtx;
    impl adk_rust::ReadonlyContext for TestCtx {
        fn invocation_id(&self) -> &str { "test" }
        fn agent_name(&self) -> &str { "momo-fetch" }
        fn user_id(&self) -> &str { "default-user" }
        fn app_name(&self) -> &str { "momo-fetch" }
        fn session_id(&self) -> &str { "test" }
        fn branch(&self) -> &str { "" }
        fn user_content(&self) -> &adk_rust::Content {
            static EMPTY: std::sync::OnceLock<adk_rust::Content> = std::sync::OnceLock::new();
            EMPTY.get_or_init(|| adk_rust::Content::new("".to_string()))
        }
    }

    let mcp = harness.mcp_service();

    // Stdio servers
    let stdio_configs = mcp.configs();
    let statuses = mcp.all_statuses().await;

    if stdio_configs.is_empty() && !mcp.has_http_servers() {
        println!("No MCP servers configured.");
        println!("Add servers to .harness/mcp.json or use /mcp add <name> <command>");
        return Ok(());
    }

    println!("MCP Connection Test");
    println!("{}", "─".repeat(50));

    // Stdio servers
    if !stdio_configs.is_empty() {
        println!("\nStdio servers:");
        for (id, config) in stdio_configs {
            let disabled = if config.disabled { " [disabled]" } else { "" };
            let status = statuses
                .get(id)
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "Unknown".to_string());
            let icon = match statuses.get(id) {
                Some(adk_tool::mcp::manager::ServerStatus::Running) => "\u{2713}".green(),
                Some(adk_tool::mcp::manager::ServerStatus::Disabled) => "\u{25CB}".yellow(),
                _ => "\u{2717}".red(),
            };
            println!("  {icon} {id}: {status}{disabled}");
            println!("    command: {} {}", config.command, config.args.join(" "));
        }
    }

    // HTTP servers
    if mcp.has_http_servers() {
        println!("\nHTTP servers:");
        let http_configs = mcp.http_configs();
        for (id, config) in http_configs {
            let icon = "\u{2713}".green(); // already connected during build
            println!("  {icon} {id} (type: {})", config.server_type);
            println!("    url: {}", config.url);
            if !config.headers.is_empty() {
                let header_keys: Vec<&str> = config.headers.keys().map(|k| k.as_str()).collect();
                println!("    headers: {}", header_keys.join(", "));
            }
        }
    }

    // Count tools
    let toolset = mcp.toolset();
    let tool_count = if let Some(ts) = &toolset {
        let ctx = Arc::new(TestCtx);
        match ts.tools(ctx).await {
            Ok(tools) => tools.len(),
            Err(e) => {
                println!("\n{} Failed to list tools: {e}", "\u{2717}".red());
                let _ = harness.mcp_service().shutdown().await;
                return Ok(());
            }
        }
    } else {
        0
    };

    println!("\n{}", "─".repeat(50));
    println!("Total tools available: {}", tool_count.to_string().green());

    let _ = harness.mcp_service().shutdown().await;
    Ok(())
}

/// Run as a memory sidecar process (Option C).
///
/// Listens for search/write requests via the Mailbox file-based protocol.
/// The main process sends messages to "memory-sidecar", and this process
/// responds back to "main".
async fn run_memory_sidecar(args: &CliArgs) -> anyhow::Result<()> {
    use crate::memory::sidecar::MemorySidecar;
    use crate::memory::vault::ObsidianVault;
    use crate::team::Mailbox;
    use crate::team::sidecar_protocol;

    let project_path = match &args.project {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    let mailbox_path = project_path.join(".harness").join("mailbox");
    let mailbox = Mailbox::open(&mailbox_path)?;

    let vault_path = project_path.join("memory-vault");
    let vault = ObsidianVault::open(&vault_path)?;

    let config = crate::config::MemorySettings::default();
    let sidecar = MemorySidecar::new(
        std::sync::Arc::new(std::sync::Mutex::new(vault)),
        config,
    );

    // Signal ready
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    mailbox.send(crate::team::MailboxMessage {
        from: sidecar_protocol::SIDECAR_ID.to_string(),
        to: sidecar_protocol::MAIN_ID.to_string(),
        msg_type: sidecar_protocol::msg_type::READY.to_string(),
        body: "Memory sidecar ready".to_string(),
        timestamp: now_ms,
    })?;

    eprintln!("Memory sidecar started (vault: {})", vault_path.display());

    // Main loop: poll mailbox for requests
    loop {
        let messages = mailbox.receive(sidecar_protocol::SIDECAR_ID)?;
        for msg in messages {
            match msg.msg_type.as_str() {
                sidecar_protocol::msg_type::SEARCH_REQUEST => {
                    let result = sidecar.search_for_context(&msg.body);
                    let response_body = if result.count > 0 {
                        serde_json::json!({
                            "context_block": result.context_block,
                            "count": result.count,
                        }).to_string()
                    } else {
                        serde_json::json!({"context_block": "", "count": 0}).to_string()
                    };

                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis() as u64;
                    mailbox.send(crate::team::MailboxMessage {
                        from: sidecar_protocol::SIDECAR_ID.to_string(),
                        to: sidecar_protocol::MAIN_ID.to_string(),
                        msg_type: sidecar_protocol::msg_type::SEARCH_RESPONSE.to_string(),
                        body: response_body,
                        timestamp: now_ms,
                    })?;
                }
                sidecar_protocol::msg_type::WRITE_REQUEST => {
                    // Parse turn summary from request body
                    let turn: serde_json::Value = serde_json::from_str(&msg.body)
                        .unwrap_or(serde_json::Value::Null);

                    let turn_summary = crate::memory::sidecar::TurnSummary {
                        user_message: turn.get("user_message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        tool_calls: turn.get("tool_calls")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect())
                            .unwrap_or_default(),
                        response_preview: turn.get("response_preview")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        project: turn.get("project")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default")
                            .to_string(),
                    };

                    let response_body = match sidecar.write_turn_memory_option_a(&turn_summary) {
                        Ok(memcell_ref) => serde_json::json!({
                            "status": "written",
                            "memcell_ref": memcell_ref,
                        }).to_string(),
                        Err(e) => serde_json::json!({
                            "status": "error",
                            "error": e.to_string(),
                        }).to_string(),
                    };

                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis() as u64;
                    mailbox.send(crate::team::MailboxMessage {
                        from: sidecar_protocol::SIDECAR_ID.to_string(),
                        to: sidecar_protocol::MAIN_ID.to_string(),
                        msg_type: sidecar_protocol::msg_type::WRITE_RESPONSE.to_string(),
                        body: response_body,
                        timestamp: now_ms,
                    })?;
                }
                sidecar_protocol::msg_type::SHUTDOWN => {
                    eprintln!("Memory sidecar shutting down.");
                    return Ok(());
                }
                _ => {
                    tracing::warn!("Memory sidecar: unknown message type: {}", msg.msg_type);
                }
            }
        }

        // Sleep between polls (100ms)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> CliArgs {
        CliArgs::try_parse_from(std::iter::once("momo-fetch").chain(args.iter().copied()))
            .unwrap_or_else(|e| panic!("failed to parse {args:?}: {e}"))
    }

    #[test]
    fn clap_definition_is_valid() {
        CliArgs::command().debug_assert();
    }

    /// The `team` subcommand must not cost the flat flags anything — these are
    /// every invocation shape that worked before it existed.
    #[test]
    fn flat_flags_still_parse_without_a_subcommand() {
        let bare = parse(&[]);
        assert!(bare.command.is_none());
        assert!(bare.prompt.is_none());
        assert_eq!(bare.permission, "strict");
        assert_eq!(bare.mode, "repl");

        let oneshot = parse(&["-p", "Explain this code"]);
        assert!(oneshot.command.is_none());
        assert_eq!(oneshot.prompt.as_deref(), Some("Explain this code"));

        assert!(parse(&["--test-mcp"]).test_mcp);
        assert!(parse(&["--gateway"]).gateway);

        let gateway = parse(&[
            "--gateway",
            "--gateway-port",
            "0",
            "--gateway-bind",
            "127.0.0.1",
            "--gateway-allow-origin",
            "tauri://localhost",
        ]);
        assert_eq!(gateway.gateway_port, Some(0));
        assert_eq!(gateway.gateway_allow_origin, vec!["tauri://localhost"]);
        assert!(gateway.command.is_none());

        let full = parse(&[
            "--project", "/repo",
            "--model", "claude-opus-5",
            "--provider", "anthropic",
            "--permission", "yolo",
            "--resume", "sess-1",
            "--mode", "memory-sidecar",
            "-a", "reviewer",
        ]);
        assert_eq!(full.project.as_deref(), Some("/repo"));
        assert_eq!(full.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(full.provider.as_deref(), Some("anthropic"));
        assert_eq!(full.permission, "yolo");
        assert_eq!(full.resume.as_deref(), Some("sess-1"));
        assert_eq!(full.mode, "memory-sidecar");
        assert_eq!(full.agent.as_deref(), Some("reviewer"));
        assert!(full.command.is_none());

        // A prompt that happens to read like a subcommand is still a prompt.
        assert_eq!(parse(&["-p", "team status"]).prompt.as_deref(), Some("team status"));
    }

    #[test]
    fn team_subcommand_parses_with_its_own_project_flag() {
        let args = parse(&["team", "status", "--project", "/repo"]);
        let Some(Command::Team { action }) = &args.command else {
            panic!("expected a team command");
        };
        match action {
            team_cmd::TeamAction::Status { scope } => {
                assert_eq!(scope.project.as_deref(), Some("/repo"));
            }
            other => panic!("expected status, got {other:?}"),
        }
        // The top-level flag was not used, so it stays empty and the fallback
        // in `run` has nothing to contribute.
        assert!(args.project.is_none());
    }

    #[test]
    fn team_subcommand_accepts_the_top_level_project_flag_too() {
        let args = parse(&["--project", "/repo", "team", "status"]);
        assert_eq!(args.project.as_deref(), Some("/repo"));
        assert!(matches!(
            args.command,
            Some(Command::Team { action: team_cmd::TeamAction::Status { .. } })
        ));
    }

    #[test]
    fn team_start_requires_a_config_name() {
        assert!(CliArgs::try_parse_from(["momo-fetch", "team", "start"]).is_err());
        let args = parse(&["team", "start", "squad"]);
        assert!(matches!(
            args.command,
            Some(Command::Team { action: team_cmd::TeamAction::Start { .. } })
        ));
    }

    #[test]
    fn team_worker_flags_parse() {
        let args = parse(&[
            "--team-worker", "validator",
            "--mailbox", "/repo/.harness/mailbox",
            "--permission", "auto",
            "-p", "stand by",
        ]);
        assert_eq!(args.team_worker.as_deref(), Some("validator"));
        assert_eq!(args.mailbox.as_deref(), Some("/repo/.harness/mailbox"));
        assert_eq!(args.prompt.as_deref(), Some("stand by"));
        assert!(args.command.is_none());

        // Neither flag is required, and their absence is the old behaviour.
        let plain = parse(&["-p", "hi"]);
        assert!(plain.team_worker.is_none());
        assert!(plain.mailbox.is_none());
    }

    #[test]
    fn team_send_and_restart_parse() {
        let args = parse(&["team", "send", "analyst", "BTC 1h", "--type", "task"]);
        let Some(Command::Team { action: team_cmd::TeamAction::Send {
            worker, message, msg_type, ..
        } }) = args.command else {
            panic!("expected a team send command");
        };
        assert_eq!(worker, "analyst");
        assert_eq!(message, "BTC 1h");
        assert_eq!(msg_type, "task");

        // --type defaults rather than being required.
        let args = parse(&["team", "send", "analyst", "go"]);
        let Some(Command::Team { action: team_cmd::TeamAction::Send { msg_type, .. } }) =
            args.command
        else {
            panic!("expected a team send command");
        };
        assert_eq!(msg_type, "task");

        let args = parse(&["team", "restart", "executor", "--project", "/repo"]);
        let Some(Command::Team { action: team_cmd::TeamAction::Restart { worker, scope } }) =
            args.command
        else {
            panic!("expected a team restart command");
        };
        assert_eq!(worker, "executor");
        assert_eq!(scope.project.as_deref(), Some("/repo"));

        // Both need a worker name.
        assert!(CliArgs::try_parse_from(["momo-fetch", "team", "send", "analyst"]).is_err());
        assert!(CliArgs::try_parse_from(["momo-fetch", "team", "restart"]).is_err());
    }

    #[test]
    fn team_stop_takes_force() {
        let args = parse(&["team", "stop", "--force"]);
        let Some(Command::Team { action: team_cmd::TeamAction::Stop { force, .. } }) = args.command
        else {
            panic!("expected a team stop command");
        };
        assert!(force);
    }
}
