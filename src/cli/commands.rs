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
    Kms,
    SkillList,
    McpList,
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
            &"kms" => Some(Self::Kms),
            &"skill" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" => Some(Self::SkillList),
                    _ => Some(Self::Unknown(input.to_string())),
                }
            }
            &"mcp" => {
                let sub = parts.get(1).unwrap_or(&"");
                match *sub {
                    "list" => Some(Self::McpList),
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
                    // Show current model
                    println!(
                        "Current: {}/{}",
                        harness.provider_mgr().current_provider(),
                        harness.provider_mgr().current_model_name()
                    );
                    return Ok(true);
                }
                match harness.provider_mgr_mut().switch_model(name) {
                    Ok(()) => {
                        println!(
                            "{} Switched to {}/{}",
                            "✓".green(),
                            harness.provider_mgr().current_provider(),
                            harness.provider_mgr().current_model_name()
                        );
                        Ok(true)
                    }
                    Err(e) => {
                        println!("{} {e}", "✗".red());
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
                match harness.provider_mgr_mut().switch_provider(name) {
                    Ok(()) => {
                        println!(
                            "{} Switched to {}/{}",
                            "✓".green(),
                            harness.provider_mgr().current_provider(),
                            harness.provider_mgr().current_model_name()
                        );
                        Ok(true)
                    }
                    Err(e) => {
                        println!("{} {e}", "✗".red());
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
                        " ← current"
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
                println!("Memory: not yet implemented");
                Ok(true)
            }
            Self::Kms => {
                println!("KMS: not yet implemented");
                Ok(true)
            }
            Self::SkillList => {
                println!("Skills: not yet implemented");
                Ok(true)
            }
            Self::McpList => {
                println!("MCP: not yet implemented");
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
                                let marker = if s.id == current_id { " ← current" } else { "" };
                                println!("  {s}{marker}");
                            }
                        }
                    }
                    Err(e) => println!("{} Failed to list sessions: {e}", "✗".red()),
                }
                Ok(true)
            }
            Self::Resume { id } => {
                if id.is_empty() {
                    println!("{} Usage: /resume <session-id>", "✗".red());
                    return Ok(true);
                }
                match harness.resume_session(id).await {
                    Ok(()) => {
                        println!(
                            "{} Resumed session {}",
                            "✓".green(),
                            harness.current_session_id()
                        );
                    }
                    Err(e) => println!("{} Failed to resume session: {e}", "✗".red()),
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
  /mcp list            List MCP servers
  /quit                Exit
  !<command>           Run shell command directly"#
    }
}
