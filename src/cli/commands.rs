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
        match self {
            Self::Help => {
                println!("{}", Self::help_text());
                Ok(true)
            }
            Self::Quit => Ok(false),
            Self::Model { name } => {
                let provider = harness.provider_mgr().current_provider().to_string();
                harness.provider_mgr_mut().switch(&provider, name);
                println!("Switched to model: {name}");
                Ok(true)
            }
            Self::Provider { name } => {
                let model = harness.provider_mgr().current_model_name().to_string();
                harness.provider_mgr_mut().switch(name, &model);
                println!("Switched to provider: {name}");
                Ok(true)
            }
            Self::Models => {
                println!("Available providers/models:");
                for (provider, model) in harness.provider_mgr().list_available() {
                    println!("  {provider}: {model}");
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
                println!("Sessions: not yet implemented");
                Ok(true)
            }
            Self::Resume { id } => {
                println!("Resume session {id}: not yet implemented");
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
  /model <name>        Switch model
  /provider <name>     Switch provider
  /models              List available models
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
