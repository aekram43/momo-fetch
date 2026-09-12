use std::io::{IsTerminal, Write};

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

    run_turn(harness, &full_prompt).await.map(|_| ())
}

/// Run one turn the way one-shot mode does — printing text to stdout and tool
/// calls to stderr — and return what the agent said.
///
/// Shared with the team-worker loop, which runs turn after turn against the
/// same harness and needs the reply text to send back to the lead.
pub async fn run_turn(harness: &Harness, full_prompt: &str) -> anyhow::Result<String> {
    // Cost lifecycle. One-shot is a third caller alongside the REPL and the
    // gateway, and it was silently missing this — `momo-fetch -p …` spent real
    // money and recorded nothing, so `/cost` under-reported by every scripted
    // invocation. Same three shared helpers the other two paths use, so the
    // three cannot drift.
    harness.begin_turn();

    // Run a single turn (with memory enrichment if auto_search is enabled)
    match harness.run_turn_enriched(full_prompt).await {
        Ok((_enriched, stream)) => {
            let (success, reply) = consume_stream_oneshot(harness, stream).await;
            harness.end_turn();
            if success {
                // Post-turn auto-write memory
                if harness.memory_sidecar().auto_write_enabled() {
                    let project_name = harness
                        .config()
                        .project_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    let turn_summary = crate::memory::sidecar::TurnSummary {
                        user_message: full_prompt.to_string(),
                        tool_calls: Vec::new(),
                        response_preview: String::new(),
                        project: project_name,
                    };

                    let _ = harness.memory_sidecar().write_turn_memory(
                        &turn_summary,
                        harness.provider_mgr(),
                        Some(&harness.mailbox_path()),
                        None,
                    );
                }
                Ok(reply)
            } else {
                // Stream completed but with errors
                Err(anyhow::anyhow!("Agent returned errors"))
            }
        }
        Err(e) => {
            // Nothing was streamed, so there is no usage to persist — but the
            // turn accumulator must not leak into whatever runs next.
            harness.end_turn();
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
async fn consume_stream_oneshot(harness: &Harness, mut stream: EventStream) -> (bool, String) {
    let mut success = true;
    let mut in_tool_call = false;
    let mut reply = String::new();

    loop {
        match stream.next().await {
            Some(Ok(event)) => {
                // Display content parts
                if let Some(content) = event.content() {
                    for part in &content.parts {
                        match part {
                            Part::Text { text } => {
                                // Agent text output goes to stdout (for piping)
                                reply.push_str(text);
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
                                        let end = summary.ceil_char_boundary(200);
                                        format!("{}...", &summary[..end])
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

                // Accumulate token usage for this turn.
                if let Some(ref usage) = event.llm_response.usage_metadata {
                    harness.record_usage(usage);
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

    (success, reply)
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
                                let end = s.ceil_char_boundary(37);
                                format!("\"{}...\"", &s[..end])
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
///
/// `std::io::IsTerminal`, not a hand-rolled trait over `libc::isatty`. The old
/// one did not compile for `x86_64-pc-windows-msvc` at all: `STDIN_FILENO` is
/// defined only under libc's `windows/gnu` module, so the Windows release build
/// failed here — which is where the desktop CI matrix died. std has had this
/// since 1.70, the crate requires 1.85, and it needs no `unsafe`.
fn atty_check() -> bool {
    std::io::stdin().is_terminal()
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
