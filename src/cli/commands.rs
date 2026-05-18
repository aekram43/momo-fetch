use crate::harness::Harness;

/// Slash command dispatch.
pub enum Command {
    Help,
    Model { name: String },
    Provider { name: String },
    Models,
    Sessions,
    Resume { id: String },
    CostToday,
    CostWeek,
    CostSession,
    CostProject { name: String },
    CostProjects,
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
    AgentList,
    AgentShow { name: String },
    AgentSwitch { name: String },
    AgentDefault,
    TeamStart { config: Option<String> },
    TeamStatus,
    TeamStop,
    TeamMerge,
    Permission { mode: String },
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
            &"cost" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "today" => Some(Self::CostToday),
                    "week" => Some(Self::CostWeek),
                    "project" => {
                        let name = parts.get(2).unwrap_or(&"").to_string();
                        if name.is_empty() {
                            // /cost project with no arg — list all projects
                            Some(Self::CostProjects)
                        } else {
                            Some(Self::CostProject { name })
                        }
                    }
                    "session" | "" => Some(Self::CostSession),
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
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
            &"agent" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" | "" => Some(Self::AgentList),
                    "show" => {
                        let name = parts.get(2).unwrap_or(&"").to_string();
                        if name.is_empty() {
                            Some(Self::Unknown("/agent show <name>".to_string()))
                        } else {
                            Some(Self::AgentShow { name })
                        }
                    }
                    "switch" | "use" => {
                        let name = parts.get(2).unwrap_or(&"").to_string();
                        if name.is_empty() {
                            Some(Self::Unknown("/agent switch <name>".to_string()))
                        } else {
                            Some(Self::AgentSwitch { name })
                        }
                    }
                    "default" | "off" | "reset" => Some(Self::AgentDefault),
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"team" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "start" => {
                        let config = parts.get(2).map(|s| s.to_string());
                        Some(Self::TeamStart { config })
                    }
                    "status" | "" => Some(Self::TeamStatus),
                    "stop" => Some(Self::TeamStop),
                    "merge" => Some(Self::TeamMerge),
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"permission" | &"perm" => Some(Self::Permission {
                mode: parts.get(1).unwrap_or(&"").to_string(),
            }),
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
            Self::CostSession => {
                let summary = harness.cost_tracker().session_summary();
                println!("Session: {}", summary);
                let today = harness.cost_tracker().today_summary();
                println!("Today:   {}", today);
                let project = harness.cost_tracker().current_project();
                if !project.is_empty() {
                    let proj = harness.cost_tracker().project_summary(&project);
                    println!("Project ({}): {}", project, proj);
                }
                Ok(true)
            }
            Self::CostToday => {
                let summary = harness.cost_tracker().today_summary();
                let session = harness.cost_tracker().session_summary();
                println!("Today:   {}", summary);
                println!("Session: {}", session);
                Ok(true)
            }
            Self::CostWeek => {
                let summary = harness.cost_tracker().week_summary();
                let session = harness.cost_tracker().session_summary();
                println!("Week:    {}", summary);
                println!("Session: {}", session);
                Ok(true)
            }
            Self::CostProject { name } => {
                let summary = harness.cost_tracker().project_summary(name);
                let session = harness.cost_tracker().session_summary();
                println!("Project ({}): {}", name, summary);
                println!("Session:      {}", session);
                Ok(true)
            }
            Self::CostProjects => {
                let projects = harness.cost_tracker().list_projects();
                if projects.is_empty() {
                    println!("No projects with cost data yet.");
                    let current = harness.cost_tracker().current_project();
                    if !current.is_empty() {
                        println!("Current project: {}", current);
                    }
                } else {
                    println!("Projects:");
                    let current = harness.cost_tracker().current_project();
                    for project in &projects {
                        let summary = harness.cost_tracker().project_summary(project);
                        let marker = if project == &current { " \u{2190} current" } else { "" };
                        println!("  {}: {}{marker}", project, summary);
                    }
                }
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
                let convention_names = [
                    "AGENTS.md", "AGENT.md", "CLAUDE.md", "GEMINI.md",
                    "COPILOT.md", "SKILLS.md", "SOUL.md",
                ];
                let skills_dir_prefixes = [
                    std::path::Path::new(".skills"),
                    std::path::Path::new(".claude/skills"),
                    std::path::Path::new(".harness/skills"),
                ];

                let (conventions, real_skills): (Vec<_>, Vec<_>) = skills.iter().partition(|s| {
                    let is_convention_name = s.path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|name| {
                            convention_names.iter().any(|c| name.eq_ignore_ascii_case(c))
                        });
                    let in_skills_dir = skills_dir_prefixes.iter().any(|prefix| {
                        s.path.starts_with(prefix)
                    });
                    is_convention_name && !in_skills_dir
                });

                if real_skills.is_empty() && conventions.is_empty() {
                    println!("No skills installed.");
                    println!("Use /skill install <git-url> to install a skill.");
                    println!("Skills can also be placed in .skills/ or .claude/skills/ or .harness/skills/");
                } else {
                    if !real_skills.is_empty() {
                        println!("Installed skills ({}):", real_skills.len());
                        for skill in &real_skills {
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
                    if !conventions.is_empty() {
                        if !real_skills.is_empty() {
                            println!();
                        }
                        println!("Project instructions ({}):", conventions.len());
                        for skill in &conventions {
                            println!("  {}: {}", skill.name, skill.path.display());
                        }
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
                use colored::Colorize;
                let configs = harness.mcp_service().configs();
                let http_configs = harness.mcp_service().http_configs();
                if configs.is_empty() && http_configs.is_empty() {
                    println!("No MCP servers configured.");
                    println!("Use /mcp add <name> <command> to add a server.");
                } else {
                    if !configs.is_empty() {
                        println!("Stdio servers:");
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
                    if !http_configs.is_empty() {
                        if !configs.is_empty() {
                            println!();
                        }
                        println!("HTTP servers:");
                        let connected = harness.mcp_service().connected_http_ids();
                        for (id, config) in http_configs {
                            let status = if connected.get(id).copied().unwrap_or(false) {
                                "Connected".green().to_string()
                            } else {
                                "Disconnected".red().to_string()
                            };
                            println!("  {id}: {status}");
                            println!("    url: {}", config.url);
                        }
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
            Self::AgentList => {
                let registry = harness.agent_registry();
                if registry.is_empty() {
                    println!("No agent personalities found.");
                    println!("Create one with: mkdir -p .harness/agents && echo 'Your prompt' > .harness/agents/researcher.md");
                } else {
                    println!("Agent personalities ({}):", registry.len());
                    for def in registry.list() {
                        let desc = def.description.as_deref().unwrap_or("(no description)");
                        let model_info = match (&def.model, &def.provider) {
                            (Some(m), Some(p)) => format!(" [{p}/{m}]"),
                            (Some(m), None) => format!(" [{m}]"),
                            _ => String::new(),
                        };
                        let caps = if def.capabilities.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", def.capabilities.join(", "))
                        };
                        println!("  {}{}{}: {}", def.name, model_info, caps, desc);
                    }
                }
                Ok(true)
            }
            Self::AgentShow { name } => {
                let registry = harness.agent_registry();
                match registry.get(name) {
                    Some(def) => {
                        println!("Agent: {}", def.name);
                        if let Some(desc) = &def.description {
                            println!("Description: {}", desc);
                        }
                        if let Some(model) = &def.model {
                            println!("Model: {}", model);
                        }
                        if let Some(provider) = &def.provider {
                            println!("Provider: {}", provider);
                        }
                        if let Some(tools) = &def.tools {
                            println!("Tools: {}", tools.join(", "));
                        }
                        if !def.capabilities.is_empty() {
                            println!("Capabilities: {}", def.capabilities.join(", "));
                        }
                        println!();
                        println!("--- Personality ---");
                        println!("{}", def.personality);
                    }
                    None => {
                        println!("{} Agent '{}' not found.", "\u{2717}".red(), name);
                        let available: Vec<_> = registry.list().iter().map(|a| a.name.clone()).collect();
                        if !available.is_empty() {
                            println!("Available: {}", available.join(", "));
                        }
                    }
                }
                Ok(true)
            }
            Self::AgentSwitch { name } => {
                match harness.switch_agent(name) {
                    Ok(()) => {
                        let agent_name = harness.config().agent_name.as_deref().unwrap_or("default");
                        println!(
                            "{} Switched to agent '{}'",
                            "\u{2713}".green(),
                            agent_name
                        );
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::AgentDefault => {
                match harness.clear_agent() {
                    Ok(()) => {
                        println!(
                            "{} Switched to default mode (no agent personality)",
                            "\u{2713}".green()
                        );
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::TeamStart { config: config_name } => {
                use crate::team::WorkerDef;

                // If a config name was provided, load from file
                let workers: Vec<WorkerDef> = if let Some(name) = config_name {
                    match harness.team_service().load_team_config(&name) {
                        Ok(team_config) => {
                            println!(
                                "{} Loaded team config '{}' ({} workers)",
                                "\u{2713}".green(),
                                name,
                                team_config.workers.len()
                            );
                            for w in &team_config.workers {
                                let agent_tag = match &w.agent {
                                    Some(a) => format!(" [agent: {a}]"),
                                    None => String::new(),
                                };
                                let branch_tag = match &w.branch {
                                    Some(b) => format!(" [branch: {b}]"),
                                    None => String::new(),
                                };
                                let wt_tag = if w.worktree.unwrap_or(false) { " [worktree]" } else { "" };
                                println!("  - {}{}{}{}: {}", w.name, agent_tag, branch_tag, wt_tag, w.task);
                            }
                            crate::team::TeamService::config_to_workers(&team_config)
                        }
                        Err(e) => {
                            println!("{} {e}", "\u{2717}".red());
                            return Ok(true);
                        }
                    }
                } else {
                    // No config name — check if .harness/teams/ has configs to offer
                    let available = harness.team_service().list_team_configs().unwrap_or_default();
                    if !available.is_empty() {
                        println!("Available team configs:");
                        for name in &available {
                            println!("  /team start {name}");
                        }
                        println!();
                    }

                    // Interactive mode
                    println!("Define worker agents (one per line, empty line to finish):");
                    println!("  Format: <name> <task> [--agent <personality>] [--branch <name>] [--worktree]");
                    println!();

                    let mut workers = Vec::new();
                    loop {
                        print!("  worker[{}]: ", workers.len());
                        use std::io::Write;
                        let _ = std::io::stdout().flush();

                        let mut line = String::new();
                        if std::io::stdin().read_line(&mut line).is_err() || line.trim().is_empty() {
                            break;
                        }

                        let line = line.trim();
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        if parts.len() < 2 {
                            println!("    {} Invalid format. Use: <name> <task>", "\u{2717}".red());
                            continue;
                        }

                        let name = parts[0].to_string();
                        let rest = parts[1];

                        // Parse optional flags
                        let mut branch = None;
                        let mut use_worktree = false;
                        let mut agent = None;

                        let rest_parts: Vec<&str> = rest.split("--").collect();
                        let task = rest_parts[0].trim().to_string();

                        for flag_part in rest_parts.iter().skip(1) {
                            let flag = flag_part.trim();
                            if flag.starts_with("branch ") {
                                branch = Some(flag[7..].trim().to_string());
                            } else if flag.starts_with("branch=") {
                                branch = Some(flag[7..].trim().to_string());
                            } else if flag.starts_with("agent ") {
                                agent = Some(flag[6..].trim().to_string());
                            } else if flag.starts_with("agent=") {
                                agent = Some(flag[6..].trim().to_string());
                            } else if flag == "worktree" {
                                use_worktree = true;
                            }
                        }

                        workers.push(WorkerDef {
                            name,
                            task,
                            branch,
                            use_worktree: if use_worktree { Some(true) } else { None },
                            agent,
                        });
                    }
                    workers
                };

                if workers.is_empty() {
                    println!("{} No workers defined. Team not started.", "\u{2717}".red());
                    return Ok(true);
                }

                // Check tmux availability
                if !crate::team::TmuxManager::is_available() {
                    println!(
                        "{} tmux not found. Workers will run in background without pane isolation.",
                        "\u{26a0}".yellow()
                    );
                }

                // Check git availability for worktrees
                let mut workers = workers;
                let has_worktree = workers.iter().any(|w| w.use_worktree.unwrap_or(false));
                if has_worktree && !crate::team::WorktreeManager::is_git_repo(harness.sandbox().root()) {
                    println!(
                        "{} Not a git repository. Worktrees disabled for all workers.",
                        "\u{26a0}".yellow()
                    );
                    for w in &mut workers {
                        w.use_worktree = None;
                    }
                }

                match harness.team_service_mut().start(workers) {
                    Ok(team_id) => {
                        println!(
                            "{} Team '{team_id}' started",
                            "\u{2713}".green()
                        );
                        let state = harness.team_service().state().unwrap();
                        println!("  Workers: {}", state.workers.len());
                        for (_, w) in &state.workers {
                            let wt = if w.use_worktree { " [worktree]" } else { "" };
                            let pane = if w.pane_id.is_some() { " [tmux]" } else { "" };
                            println!("    {} (branch: {}){wt}{pane}", w.name, w.branch);
                        }
                        println!("  Use /team status to check progress");
                        println!("  Use /team merge to merge completed workers");
                        println!("  Use /team stop to stop all workers");
                    }
                    Err(e) => {
                        println!("{} Failed to start team: {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::TeamStatus => {
                let status = harness.team_service_mut().status();
                match status {
                    crate::team::TeamStatus::Idle => {
                        println!("No active team.");
                        println!("Use /team start to create a team.");
                    }
                    crate::team::TeamStatus::Running | crate::team::TeamStatus::Completed => {
                        let state = harness.team_service().state().unwrap();
                        println!("Team: {}", state.id);
                        println!("Status: {}", state.status);

                        let mailbox = crate::team::Mailbox::open(&state.mailbox_path);
                        let unread = mailbox.map(|m| m.unread_count("lead")).unwrap_or(0);
                        if unread > 0 {
                            println!("Unread messages: {unread}");
                        }

                        println!();
                        println!("Workers:");
                        for (_, w) in &state.workers {
                            let result = match &w.result {
                                Some(r) => format!(" — {}", r.chars().take(80).collect::<String>()),
                                None => String::new(),
                            };
                            println!("  {}: {}{result}", w.name, w.status);
                        }

                        if matches!(state.status, crate::team::TeamStatus::Completed) {
                            println!();
                            println!(
                                "{} All workers completed. Use /team merge to merge branches.",
                                "\u{2139}".bright_blue()
                            );
                        }
                    }
                    crate::team::TeamStatus::Stopped => {
                        println!("Team was stopped.");
                    }
                }
                Ok(true)
            }
            Self::TeamStop => {
                match harness.team_service_mut().stop() {
                    Ok(()) => {
                        println!(
                            "{} Team stopped. Workers terminated, worktrees cleaned up.",
                            "\u{2713}".green()
                        );
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::TeamMerge => {
                match harness.team_service_mut().merge() {
                    Ok(results) => {
                        if results.is_empty() {
                            println!("No completed workers with worktrees to merge.");
                        } else {
                            println!("Merge results:");
                            for r in &results {
                                let marker = if r.success {
                                    "\u{2713}".green().to_string()
                                } else {
                                    "\u{2717}".red().to_string()
                                };
                                println!("  {marker} {} ({}): {}", r.worker, r.branch, r.message);
                            }
                        }
                    }
                    Err(e) => {
                        println!("{} {e}", "\u{2717}".red());
                    }
                }
                Ok(true)
            }
            Self::Permission { mode } => {
                use crate::sandbox::PermissionMode;
                use std::str::FromStr;

                if mode.is_empty() {
                    println!(
                        "Current: {}",
                        harness.sandbox().permission_mode()
                    );
                    return Ok(true);
                }
                match PermissionMode::from_str(mode) {
                    Ok(new_mode) => {
                        match harness.switch_permission(new_mode) {
                            Ok(()) => {
                                println!(
                                    "{} Switched to {}",
                                    "\u{2713}".green(),
                                    harness.sandbox().permission_mode()
                                );
                            }
                            Err(e) => {
                                println!("{} Failed to switch permission mode: {e}", "\u{2717}".red());
                            }
                        }
                    }
                    Err(_) => {
                        println!(
                            "{} Unknown mode '{mode}'. Use: strict, auto, or yolo",
                            "\u{2717}".red()
                        );
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
  /cost                Show session + today + project cost
  /cost today          Show today's cost
  /cost week           Show this week's cost
  /cost project <name> Show cost for a specific project
  /cost project        List all projects with cost data
  /mem                 Memory status
  /kms                 Knowledge base status
  /skill list          List installed skills
  /skill install <url> Install skill from git URL
  /mcp list            List MCP servers
  /mcp add <n> <cmd>   Add MCP server
  /mcp remove <name>   Remove MCP server
  /key set <provider>  Store API key in OS keychain
  /key list            List stored providers (keys masked)
  /key delete <name>   Delete API key from keychain
  /agent list          List agent personalities
  /agent show <name>   Show agent personality details
  /agent switch <name> Switch to an agent personality mid-session
  /agent default       Switch back to default mode (no personality)
  /team start [name]   Start a team (from config or interactive)
  /team status         Show team status and worker progress
  /team merge          Merge completed workers' branches
  /team stop           Stop team and clean up worktrees
  /permission [mode]   Switch permission mode (strict/auto/yolo)
  /quit                Exit
  !<command>           Run shell command directly"#
    }
}
