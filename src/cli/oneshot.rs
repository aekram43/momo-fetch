use std::io::Write;

use adk_rust::futures::StreamExt;
use adk_rust::{EventStream, Part};
use colored::Colorize;

use crate::harness::Harness;

/// Run a single prompt and exit.
///
/// - stdout: agent response text
/// - stderr: debug/logging (via tracing)
/// - exit code 0 on success, 1 on error
pub async fn run(harness: &Harness, prompt: &str) -> anyhow::Result<()> {
    // Check for stdin pipe
    let mut full_prompt = prompt.to_string();
    if !atty_check() {
        let mut stdin_content = String::new();
        if std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_content).is_ok()
            && !stdin_content.is_empty()
        {
            full_prompt = format!("{prompt}\n\n---\n{stdin_content}");
        }
    }

    // Run a single turn
    match harness.run_turn(&full_prompt).await {
        Ok(stream) => {
            let success = consume_stream_oneshot(stream).await;
            if success {
                Ok(())
            } else {
                // Stream completed but with errors
                Err(anyhow::anyhow!("Agent returned errors"))
            }
        }
        Err(e) => {
            eprintln!("{} {e}", "Error:".red());
            Err(e)
        }
    }
}

/// Consume an EventStream for one-shot mode.
///
/// In one-shot mode we:
/// - Print tool calls to stderr (yellow, dimmed)
/// - Print text output to stdout
/// - Return false if any errors occurred
async fn consume_stream_oneshot(mut stream: EventStream) -> bool {
    let mut success = true;
    let mut in_tool_call = false;

    loop {
        match stream.next().await {
            Some(Ok(event)) => {
                // Display content parts
                if let Some(content) = event.content() {
                    for part in &content.parts {
                        match part {
                            Part::Text { text } => {
                                // Agent text output goes to stdout (for piping)
                                print!("{}", text);
                                let _ = std::io::stdout().flush();
                                in_tool_call = false;
                            }
                            Part::FunctionCall { name, args, .. } => {
                                // Tool calls go to stderr (don't pollute stdout)
                                if in_tool_call {
                                    eprintln!();
                                }
                                eprintln!(
                                    "  {} {}({})",
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
                                    let truncated = if summary.len() > 200 {
                                        format!("{}...", &summary[..200])
                                    } else {
                                        summary
                                    };
                                    eprintln!(
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
                        eprintln!("{} {err}", "Error:".red());
                        success = false;
                    }
                }

                // Check if turn is complete
                if event.is_final_response() {
                    break;
                }
            }
            Some(Err(e)) => {
                eprintln!("{} Stream error: {e}", "\u{2717}".red());
                success = false;
                break;
            }
            None => break,
        }
    }

    // Ensure final newline on stdout
    println!();

    success
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
    if let serde_json::Value::String(inner) = response {
        inner.clone()
    } else {
        response.to_string()
    }
}

/// Check if stdin is a terminal (not piped).
fn atty_check() -> bool {
    std::io::stdin().is_terminal()
}

trait IsTerminal {
    fn is_terminal(&self) -> bool;
}

impl IsTerminal for std::io::Stdin {
    fn is_terminal(&self) -> bool {
        unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_args_object() {
        let args = serde_json::json!({
            "path": "src/main.rs",
            "range": "1-50"
        });
        let summary = summarize_args(&args);
        assert!(summary.contains("path="));
        assert!(summary.contains("range="));
    }

    #[test]
    fn test_summarize_args_long_string() {
        let args = serde_json::json!({
            "content": "a".repeat(100)
        });
        let summary = summarize_args(&args);
        assert!(summary.contains("..."));
        assert!(summary.len() < 100);
    }

    #[test]
    fn test_summarize_response_string() {
        let response = serde_json::json!("success");
        assert_eq!(summarize_response(&response), "success");
    }

    #[test]
    fn test_summarize_response_object() {
        let response = serde_json::json!({"success": true});
        let summary = summarize_response(&response);
        assert!(summary.contains("success"));
    }
}
