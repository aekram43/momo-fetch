mod banner;
mod commands;
mod oneshot;
mod repl;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "momo-fetch", version, about = "MOMO Fetch — AI coding companion")]
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

    /// Run mode: "repl" (default) or "memory-sidecar" (Option C: separate process sidecar)
    #[arg(long = "mode", default_value = "repl")]
    pub mode: String,
}

/// Main CLI entry point.
pub async fn run(args: CliArgs) -> anyhow::Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Check for memory-sidecar mode (Option C)
    if args.mode == "memory-sidecar" {
        return run_memory_sidecar(&args).await;
    }

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
