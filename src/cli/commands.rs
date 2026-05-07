use crate::harness::Harness;

/// Slash command dispatch.
pub enum Command {
    Help,
    Model { name: String },
    Provider { name: String },
    Models,
    Sessions,
    Resume { id: String },
    Cost,
    Mem,
    KmsList,
    SkillList,
    SkillInstall { git_url: String },
    McpList,
    McpAdd { name: String, command: String },
    McpRemove { name: String },
    KeySet { provider: String },
    KeyList,
    KeyDelete { provider: String },
    Quit,
    ShellEscape { command: String },
    Unknown(String),
}

impl Command {
    /// Parse a slash command from user input.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if !input.starts_with('/') && !input.starts_with('!') {
            return None;
        }

        if input.starts_with('!') {
            let cmd = input[1..].trim().to_string();
            return Some(Self::ShellEscape { command: cmd });
        }

        let parts: Vec<&str> = input[1..].splitn(3, ' ').collect();
        match parts.first()? {
            &"help" => Some(Self::Help),
            &"quit" | &"exit" => Some(Self::Quit),
            &"model" => Some(Self::Model {
                name: parts.get(1).unwrap_or(&"").to_string(),
            }),
            &"provider" => Some(Self::Provider {
                name: parts.get(1).unwrap_or(&"").to_string(),
            }),
            &"models" => Some(Self::Models),
            &"sessions" => Some(Self::Sessions),
            &"resume" => Some(Self::Resume {
                id: parts.get(1).unwrap_or(&"").to_string(),
            }),
            &"cost" => Some(Self::Cost),
            &"mem" => Some(Self::Mem),
            &"kms" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" | "" => Some(Self::KmsList),
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"skill" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" => Some(Self::SkillList),
                    "install" => {
                        let git_url = parts.get(2).unwrap_or(&"").to_string();
                        if git_url.is_empty() {
                            Some(Self::Unknown("/skill install <git-url>".to_string()))
                        } else {
                            Some(Self::SkillInstall { git_url })
                        }
                    }
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"mcp" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" => Some(Self::McpList),
                    "add" => {
                        // /mcp add <name> <command> [args...]
                        let name = parts.get(2).unwrap_or(&"").to_string();
                        // Everything after "mcp add <name>" is the command
                        let rest = input[1..].trim_start_matches("mcp add").trim();
                        // Skip the name part to get the command
                        let cmd = if let Some(space_pos) = rest.find(' ') {
                            rest[space_pos..].trim().to_string()
                        } else {
                            String::new()
                        };
                        if name.is_empty() || cmd.is_empty() {
                            Some(Self::Unknown(
                                "/mcp add <name> <command> [args...]".to_string(),
                            ))
                        } else {
                            Some(Self::McpAdd {
                                name,
                                command: cmd,
                            })
                        }
                    }
                    "remove" => {
                        let name = parts.get(2).unwrap_or(&"").to_string();
                        if name.is_empty() {
                            Some(Self::Unknown("/mcp remove <name>".to_string()))
                        } else {
                            Some(Self::McpRemove { name })
                        }
                    }
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"key" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "set" => {
                        let provider = parts.get(2).unwrap_or(&"").to_string();
                        if provider.is_empty() {
                            Some(Self::Unknown("/key set <provider>".to_string()))
                        } else {
                            Some(Self::KeySet { provider })
                        }
                    }
                    "list" | "" => Some(Self::KeyList),
                    "delete" | "rm" => {
                        let provider = parts.get(2).unwrap_or(&"").to_string();
                        if provider.is_empty() {
                            Some(Self::Unknown("/key delete <provider>".to_string()))
                        } else {
                            Some(Self::KeyDelete { provider })
                        }
                    }
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            _ => Some(Self::Unknown(input.to_string())),
        }
    }

    /// Execute the command against the harness.
    pub async fn execute(&self, harness: &mut Harness) -> anyhow::Result<bool> {
        use colored::Colorize;

        match self {
            Self::Help => {
                println!("{}", Self::help_text());
                Ok(true)
            }
            Self::Quit => Ok(false),
            Self::Model { name } => {
                if name.is_empty() {
                    println!(
                        "Current: {}/{}",
                        harness.provider_mgr().current_provider(),
                        harness.provider_mgr().current_model_name()
                    );
                    return Ok(true);
                }
                match harness.switch_model(name) {
                    Ok(()) => {
                        println!(
                            "{} Switched to {}/{}",
                            "\u{2713}".green(),
                            harness.provider_mgr().current_provider(),
                            harness.provider_mgr().current_model_name()
                        );
                        Ok(true)
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                        Ok(true)
                    }
                }
            }
            Self::Provider { name } => {
                if name.is_empty() {
                    println!(
                        "Current: {}/{}",
                        harness.provider_mgr().current_provider(),
                        harness.provider_mgr().current_model_name()
                    );
                    return Ok(true);
                }
                match harness.switch_provider(name) {
                    Ok(()) => {
                        println!(
                            "{} Switched to {}/{}",
                            "\u{2713}".green(),
                            harness.provider_mgr().current_provider(),
                            harness.provider_mgr().current_model_name()
                        );
                        Ok(true)
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                        Ok(true)
                    }
                }
            }
            Self::Models => {
                println!("Available providers/models:");
                let current_provider = harness.provider_mgr().current_provider();
                let current_model = harness.provider_mgr().current_model_name();
                for info in harness.provider_mgr().list_available() {
                    let marker = if info.provider == current_provider
                        && info.default_model == current_model
                    {
                        " \u{2190} current"
                    } else {
                        ""
                    };
                    println!("  {}: {}{marker}", info.provider, info.default_model);
                }
                Ok(true)
            }
            Self::Cost => {
                println!("Cost tracking: not yet implemented");
                Ok(true)
            }
            Self::Mem => {
                let vault_arc = harness.vault();
                let vault = vault_arc.lock().map_err(|e| anyhow::anyhow!("vault lock: {e}"))?;
                let stats = vault.stats();
                let counters = vault.counters();

                println!("Memory Vault Status:");
                println!("  MemCells:    {}", stats.total_memcells);
                println!("  Events:      {} (next: fact-{:04})", stats.total_events, counters.event + 1);
                println!("  Foresights:  {} ({} pending, next: pred-{:04})", stats.total_foresights, stats.pending_foresights, counters.foresight + 1);
                println!("  Episodes:    {} (next: ep-{:04})", stats.total_episodes, counters.episode + 1);
                println!("  Clusters:    {}", counters.cluster);
                println!("  Reflections: {}", counters.reflection);
                println!();
                println!("Vault path: {}", vault.path().display());
                println!();
                println!("Use mem_write and mem_extract tools to interact with the vault.");
                Ok(true)
            }
            Self::KmsList => {
                use crate::tools::kms::list_knowledge_bases;
                let kbs = list_knowledge_bases(harness.sandbox().root());
                if kbs.is_empty() {
                    println!("No knowledge bases found.");
                    println!("Create one with: mkdir -p .kms/<name>/pages");
                } else {
                    println!("Knowledge bases:");
                    for kb in &kbs {
                        let index_marker = if kb.has_index { "" } else { " (no index)" };
                        println!(
                            "  {} ({} pages){index_marker}",
                            kb.name, kb.page_count
                        );
                    }
                }
                Ok(true)
            }
            Self::SkillList => {
                let skills = harness.skill_service().index().skills();
                if skills.is_empty() {
                    println!("No skills installed.");
                    println!("Use /skill install <git-url> to install a skill.");
                    println!("Skills can also be placed in .skills/ or .claude/skills/ or .harness/skills/");
                } else {
                    println!("Installed skills ({}):", skills.len());
                    for skill in skills {
                        let trigger_marker = if skill.trigger { " [explicit]" } else { "" };
                        let version_marker = skill
                            .version
                            .as_ref()
                            .map(|v| format!(" v{v}"))
                            .unwrap_or_default();
                        let tags = if skill.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", skill.tags.join(", "))
                        };
                        println!(
                            "  {}{}: {}{}{}",
                            skill.name, version_marker, skill.description, tags, trigger_marker
                        );
                        println!("    {}", skill.path.display());
                    }
                }
                Ok(true)
            }
            Self::SkillInstall { git_url } => {
                match harness.skill_service_mut().install_from_git(git_url) {
                    Ok(name) => {
                        println!(
                            "{} Installed skill '{name}'",
                            "\u{2713}".green()
                        );
                        // Rebuild runner to include new skill context
                        if let Err(e) = harness.rebuild_runner() {
                            println!(
                                "{} Skill installed but runner rebuild failed: {e}",
                                "\u{2717}".red()
                            );
                        }
                    }
                    Err(e) => {
                        println!("{} Failed to install skill: {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::McpList => {
                let configs = harness.mcp_service().configs();
                if configs.is_empty() {
                    println!("No MCP servers configured.");
                    println!("Use /mcp add <name> <command> to add a server.");
                } else {
                    println!("MCP servers:");
                    let statuses = harness.mcp_service().all_statuses().await;
                    for (id, config) in configs {
                        let status = statuses
                            .get(id)
                            .map(|s| format!("{s:?}"))
                            .unwrap_or_else(|| "Unknown".to_string());
                        let disabled = if config.disabled { " [disabled]" } else { "" };
                        println!("  {id}: {status}{disabled}");
                        println!("    command: {} {}", config.command, config.args.join(" "));
                    }
                }
                Ok(true)
            }
            Self::McpAdd { name, command } => {
                use std::collections::HashMap;

                // Parse command string into command + args
                let parts: Vec<&str> = command.split_whitespace().collect();
                if parts.is_empty() {
                    println!("{} No command specified", "\u{2717}".red());
                    return Ok(true);
                }

                let cmd = parts[0].to_string();
                let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

                let config = adk_tool::mcp::manager::McpServerConfig {
                    command: cmd,
                    args,
                    env: HashMap::new(),
                    disabled: false,
                    auto_approve: vec![],
                    restart_policy: None,
                };

                match harness
                    .mcp_service_mut()
                    .add_server(name.clone(), config, true)
                    .await
                {
                    Ok(()) => {
                        println!(
                            "{} Added MCP server '{name}' and starting...",
                            "\u{2713}".green()
                        );
                        // Rebuild runner to include new MCP tools
                        if let Err(e) = harness.rebuild_runner() {
                            println!("{} Server added but runner rebuild failed: {e}", "\u{2717}".red());
                        }
                    }
                    Err(e) => {
                        println!("{} Failed to add MCP server '{name}': {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::McpRemove { name } => {
                match harness.mcp_service_mut().remove_server(&name).await {
                    Ok(()) => {
                        println!(
                            "{} Removed MCP server '{name}'",
                            "\u{2713}".green()
                        );
                        // Rebuild runner to remove MCP tools
                        if let Err(e) = harness.rebuild_runner() {
                            println!(
                                "{} Server removed but runner rebuild failed: {e}",
                                "\u{2717}".red()
                            );
                        }
                    }
                    Err(e) => {
                        println!(
                            "{} Failed to remove MCP server '{name}': {e}",
                            "\u{2717}".red()
                        );
                    }
                }
                Ok(true)
            }
            Self::Sessions => {
                match harness.session_mgr().list_sessions().await {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            println!("No sessions found.");
                        } else {
                            println!("Sessions:");
                            let current_id = harness.current_session_id();
                            for s in &sessions {
                                let marker =
                                    if s.id == current_id { " \u{2190} current" } else { "" };
                                println!("  {s}{marker}");
                            }
                        }
                    }
                    Err(e) => println!("{} Failed to list sessions: {e}", "\u{2717}".red()),
                }
                Ok(true)
            }
            Self::Resume { id } => {
                if id.is_empty() {
                    println!("{} Usage: /resume <session-id>", "\u{2717}".red());
                    return Ok(true);
                }
                match harness.resume_session(id).await {
                    Ok(()) => {
                        println!(
                            "{} Resumed session {}",
                            "\u{2713}".green(),
                            harness.current_session_id()
                        );
                    }
                    Err(e) => println!("{} Failed to resume session: {e}", "\u{2717}".red()),
                }
                Ok(true)
            }
            Self::ShellEscape { command } => {
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(harness.sandbox().root())
                    .output()?;
                print!("{}", String::from_utf8_lossy(&output.stdout));
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
                Ok(true)
            }
            Self::KeySet { provider } => {
                use crate::config::secrets::{SecretStore, default_model_for_provider};

                println!("Enter API key for {provider} (default model: {}):", default_model_for_provider(provider));
                println!("  (The key will be stored in your OS keychain)");

                // Read key from stdin without echo
                let key = rpassword::prompt_password("API key: ")
                    .map_err(|e| anyhow::anyhow!("Failed to read input: {e}"))?;

                if key.is_empty() {
                    println!("{} No key provided", "\u{2717}".red());
                    return Ok(true);
                }

                // Validate: warn if key looks too short
                if key.len() < 8 {
                    println!(
                        "{} Warning: key seems very short ({} chars). Proceeding anyway...",
                        "\u{26a0}".yellow(),
                        key.len()
                    );
                }

                match SecretStore::set(&provider, &key) {
                    Ok(()) => {
                        println!(
                            "{} Stored API key for '{provider}' in OS keychain",
                            "\u{2713}".green()
                        );
                        println!(
                            "  You can now use /provider {} or /model {}",
                            provider, default_model_for_provider(provider)
                        );
                    }
                    Err(e) => {
                        println!("{} Failed to store key: {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::KeyList => {
                use crate::config::secrets::{SecretStore, mask_key};

                let entries = SecretStore::list();
                if entries.is_empty() {
                    println!("No secrets found.");
                    println!("Use /key set <provider> to store an API key.");
                    println!("Or set environment variables (e.g., ANTHROPIC_API_KEY).");
                } else {
                    println!("Secrets (env vars take priority over keychain):");
                    for (provider, source) in &entries {
                        let key = SecretStore::get(provider);
                        let masked = match &key {
                            Ok(k) => format!("{} ({} chars)", mask_key(k), k.len()),
                            Err(_) => "(error reading)".to_string(),
                        };
                        let source_marker = match source.as_str() {
                            "env" => " [env]".dimmed().to_string(),
                            "keychain" => " [keychain]".dimmed().to_string(),
                            "both" => " [env+keychain]".dimmed().to_string(),
                            "none" => " [no key needed]".dimmed().to_string(),
                            _ => String::new(),
                        };
                        println!("  {provider}: {masked}{source_marker}");
                    }
                }
                Ok(true)
            }
            Self::KeyDelete { provider } => {
                use crate::config::secrets::SecretStore;

                match SecretStore::delete(provider) {
                    Ok(()) => {
                        println!(
                            "{} Deleted API key for '{provider}' from OS keychain",
                            "\u{2713}".green()
                        );
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::Unknown(cmd) => {
                println!("Unknown command: {cmd}");
                println!("Type /help for available commands.");
                Ok(true)
            }
        }
    }

    fn help_text() -> &'static str {
        r#"Available commands:
  /help                Show this help message
  /model [name]        Switch model (or show current)
  /provider [name]     Switch provider (or show current)
  /models              List available providers/models
  /sessions            List past sessions
  /resume <id>         Resume a session
  /cost                Show session cost
  /mem                 Memory status
  /kms                 Knowledge base status
  /skill list          List installed skills
  /skill install <url> Install skill from git URL
  /mcp list            List MCP servers
  /mcp add <n> <cmd>   Add MCP server (e.g., /mcp add fs npx -y @mcp/filesystem /tmp)
  /mcp remove <name>   Remove MCP server
  /key set <provider>  Store API key in OS keychain
  /key list            List stored providers (keys masked)
  /key delete <name>   Delete API key from keychain
  /quit                Exit
  !<command>           Run shell command directly"#
    }
}
