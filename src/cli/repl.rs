use crate::harness::Harness;

/// Run the interactive REPL loop.
pub async fn run(harness: &mut Harness) -> anyhow::Result<()> {
    use colored::Colorize;
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

    println!(
        "{} {} — {} ({})",
        "Agent Harness".green().bold(),
        env!("CARGO_PKG_VERSION"),
        harness.provider_mgr().current_provider(),
        harness.provider_mgr().current_model_name(),
    );
    println!("Type /help for commands, Ctrl+D to quit.\n");

    let mut rl = DefaultEditor::new()?;
    let history_path = dirs::home_dir()
        .map(|h| h.join(".config/agent-harness/history.txt"));

    if let Some(ref path) = history_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.load_history(path);
    }

    loop {
        let readline = rl.readline("harness> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                // Check for slash commands
                if let Some(cmd) = super::commands::Command::parse(trimmed) {
                    match cmd.execute(harness).await {
                        Ok(true) => continue,
                        Ok(false) => break,
                        Err(e) => eprintln!("{}", format!("Error: {e}").red()),
                    }
                    continue;
                }

                // TODO: Send to agent via adk-runner (US-008)
                println!("(Agent response not yet implemented)");
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("Goodbye!");
                break;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                break;
            }
        }
    }

    if let Some(path) = history_path {
        let _ = rl.save_history(&path);
    }

    Ok(())
}
