use std::io::Write;

use adk_rust::futures::StreamExt;
use adk_rust::{EventStream, Part};
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::harness::Harness;

/// Run the interactive REPL loop.
pub async fn run(harness: &mut Harness) -> anyhow::Result<()> {
    // Print MoMo banner
    super::banner::print_banner();

    // Print startup info
    println!(
        "{} {} \u{2014} {} ({})",
        "Agent Harness".green().bold(),
        env!("CARGO_PKG_VERSION"),
        harness.provider_mgr().current_provider(),
        harness.provider_mgr().current_model_name(),
    );
    let sid = harness.current_session_id();
    let short_id = &sid[..8.min(sid.len())];
    println!("Session: {short_id}");

    // Show loaded context files with relative paths
    let paths = harness.context_builder().loaded_file_relative_paths();
    if !paths.is_empty() {
        println!("Loaded context from: {}", paths.join(", "));
    }

    println!("Type /help for commands, Ctrl+D to quit.\n");

    let mut rl = DefaultEditor::new()?;
    let history_path =
        dirs::home_dir().map(|h| h.join(".config/agent-harness/history.txt"));

    if let Some(ref path) = history_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.load_history(path);
    }

    loop {
        let prompt = format!(
            "{}> ",
            harness.provider_mgr().current_model_name().dimmed()
        );
        let readline = rl.readline(&prompt);
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

                // Multi-line input detection
                let full_input = if trimmed.contains("```") {
                    read_multiline_code(&mut rl, trimmed)
                } else if trimmed.ends_with('\\') {
                    read_multiline_continuation(&mut rl, trimmed)
                } else {
                    trimmed.to_string()
                };

                if full_input.trim().is_empty() {
                    continue;
                }

                // Run turn with streaming
                run_turn_streaming(harness, &full_input).await;
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C during input
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

/// Read multi-line input when a code block (```) is opened but not closed.
fn read_multiline_code(rl: &mut DefaultEditor, first_line: &str) -> String {
    let mut buffer = first_line.to_string();

    // Count opening vs closing ```
    let open_count = first_line.matches("```").count();
    if open_count % 2 == 0 {
        return buffer; // Already balanced
    }

    loop {
        match rl.readline("... ") {
            Ok(line) => {
                buffer.push('\n');
                buffer.push_str(&line);
                if line.contains("```") {
                    break; // Closing ``` found
                }
            }
            Err(_) => break,
        }
    }

    buffer
}

/// Read multi-line input with backslash continuation.
fn read_multiline_continuation(rl: &mut DefaultEditor, first_line: &str) -> String {
    let mut buffer = first_line.to_string();
    buffer.pop(); // Remove trailing backslash

    loop {
        match rl.readline("... ") {
            Ok(line) => {
                if line.ends_with('\\') {
                    let line = &line[..line.len() - 1];
                    buffer.push('\n');
                    buffer.push_str(line);
                } else {
                    buffer.push('\n');
                    buffer.push_str(&line);
                    break;
                }
            }
            Err(_) => break,
        }
    }

    buffer
}

/// Run a single conversational turn with streaming output.
async fn run_turn_streaming(harness: &Harness, input: &str) {
    match harness.run_turn(input).await {
        Ok(stream) => {
            consume_stream(harness, stream).await;
        }
        Err(e) => {
            println!("{} {}", "\u{2717}".red(), format!("{e}").red());
        }
    }
}

/// Consume an EventStream with colored output and Ctrl+C cancellation.
async fn consume_stream(harness: &Harness, mut stream: EventStream) {
    let mut in_tool_call = false;
    let mut has_output = false;

    loop {
        tokio::select! {
            result = stream.next() => {
                match result {
                    Some(Ok(event)) => {
                        has_output = true;

                        // Handle tool confirmation request
                        if let Some(confirm_req) = &event.actions.tool_confirmation {
                            println!(
                                "\n  {} Tool {} requires approval: {}",
                                "!".yellow(),
                                confirm_req.tool_name.yellow(),
                                summarize_args(&confirm_req.args),
                            );
                        }

                        // Display content parts
                        if let Some(content) = event.content() {
                            for part in &content.parts {
                                match part {
                                    Part::Text { text } => {
                                        print!("{}", text);
                                        let _ = std::io::stdout().flush();
                                        in_tool_call = false;
                                    }
                                    Part::FunctionCall { name, args, .. } => {
                                        if in_tool_call {
                                            println!();
                                        }
                                        println!(
                                            "\n  {} {}({})",
                                            "\u{23fa}".yellow(),
                                            name.yellow(),
                                            summarize_args(args),
                                        );
                                        in_tool_call = true;
                                    }
                                    Part::FunctionResponse { function_response, .. } => {
                                        if in_tool_call {
                                            let summary =
                                                summarize_response(&function_response.response);
                                            // Truncate long responses
                                            let truncated = if summary.len() > 200 {
                                                format!("{}...", &summary[..200])
                                            } else {
                                                summary
                                            };
                                            println!(
                                                "  {} {}",
                                                "\u{2192}".dimmed(),
                                                truncated.dimmed(),
                                            );
                                            in_tool_call = false;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Check for errors in the response
                        if let Some(ref err) = event.llm_response.error_message {
                            if !err.is_empty() {
                                println!("\n{} {}", "Error:".red(), err.red());
                            }
                        }

                        // Check if turn is complete
                        if event.is_final_response() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        println!("\n{} Stream error: {}", "\u{2717}".red(), e);
                        break;
                    }
                    None => break,
                }
            }
            _ = tokio::signal::ctrl_c() => {
                harness.interrupt();
                println!("\n^C Generation cancelled");
                break;
            }
        }
    }

    if has_output {
        println!(); // Trailing newline after response
    }
}

/// Summarize tool arguments for display.
fn summarize_args(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .take(3)
                .map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 40 {
                                format!("\"{}...\"", &s[..37])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => format!("{other}"),
                    };
                    format!("{k}={val_str}")
                })
                .collect();
            let summary = parts.join(", ");
            if map.len() > 3 {
                format!("{summary}, ...")
            } else {
                summary
            }
        }
        other => format!("{other}"),
    }
}

/// Summarize a tool response for display.
fn summarize_response(response: &serde_json::Value) -> String {
    let s = response.to_string();
    // Remove outer quotes if it's a string
    if let serde_json::Value::String(inner) = response {
        inner.clone()
    } else {
        s
    }
}
