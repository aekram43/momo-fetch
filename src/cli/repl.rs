use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use adk_rust::futures::StreamExt;
use adk_rust::{EventStream, Part};
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::harness::Harness;
use crate::memory::sidecar::TurnSummary;

// ─── Thinking spinner ──

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⦦", "⠧", "⠇", "⠏"];
const SPINNER_LABELS: &[&str] = &["Thinking", "Analyzing", "Processing", "Generating"];

struct ThinkingSpinner {
    active: Arc<AtomicBool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl ThinkingSpinner {
    fn start() -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let active_clone = active.clone();
        let handle = tokio::spawn(async move {
            let mut frame = 0usize;
            let mut label = 0usize;
            let mut tick = 0u32;
            loop {
                if !active_clone.load(Ordering::Relaxed) {
                    break;
                }
                // Clear previous spinner line: \r + spaces + \r
                eprint!("\r\u{1b}[2K  {} {}", SPINNER_FRAMES[frame].dimmed(), SPINNER_LABELS[label].dimmed());
                let _ = std::io::stderr().flush();
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                frame = (frame + 1) % SPINNER_FRAMES.len();
                tick += 1;
                // Rotate label every 25 ticks (~2 seconds)
                if tick % 25 == 0 {
                    label = (label + 1) % SPINNER_LABELS.len();
                }
            }
            // Clear the spinner line
            eprint!("\r\u{1b}[2K");
            let _ = std::io::stderr().flush();
        });
        Self {
            active,
            handle: Some(handle),
        }
    }

    fn stop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Block until the spinner task has cleaned up its line.
            // Using block_in_place so we don't deadlock the tokio runtime
            // (we're inside a select! / async context).
            let _ = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let _ = handle.await;
                })
            });
        }
    }
}

impl Drop for ThinkingSpinner {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─── Thread-local for collecting turn data during stream consumption ──

thread_local! {
    static TURN_SUMMARY: std::cell::RefCell<Option<TurnSummary>> = std::cell::RefCell::new(None);
}

/// Run the interactive REPL loop.
pub async fn run(harness: &mut Harness) -> anyhow::Result<()> {
    // Print MoMo banner
    super::banner::print_banner();

    // Print startup info
    println!(
        "{} {} \u{2014} {} ({})",
        "MOMO Fetch".green().bold(),
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

    // Show MCP servers
    let mcp = harness.mcp_service();
    if mcp.has_servers() {
        let stdio_running = mcp.running_count().await;
        let stdio_total = mcp.configs().len();
        let http_total = mcp.http_configs().len();
        let http_connected = mcp.connected_http_ids().values().filter(|&&v| v).count();
        let total = stdio_total + http_total;
        let running = stdio_running + http_connected;
        println!(
            "MCP: {}/{} servers connected",
            running.to_string().green(),
            total,
        );
    }

    // Show skills (exclude convention files like AGENTS.md, SOUL.md)
    let skills = harness.skill_service();
    let convention_names = [
        "AGENTS.md", "AGENT.md", "CLAUDE.md", "GEMINI.md",
        "COPILOT.md", "SKILLS.md", "SOUL.md",
    ];
    let skills_dir_prefixes = [
        std::path::Path::new(".skills"),
        std::path::Path::new(".claude/skills"),
        std::path::Path::new(".harness/skills"),
    ];
    let real_skill_count = skills.index().skills().iter().filter(|s| {
        let is_convention_name = s.path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| {
                convention_names.iter().any(|c| name.eq_ignore_ascii_case(c))
            });
        let in_skills_dir = skills_dir_prefixes.iter().any(|prefix| {
            s.path.starts_with(prefix)
        });
        !is_convention_name || in_skills_dir
    }).count();
    if real_skill_count > 0 {
        println!(
            "Skills: {} loaded",
            real_skill_count.to_string().green(),
        );
    }

    println!("Type /help for commands, Ctrl+D to quit.\n");

    // Graceful shutdown state: set to true when user wants to exit
    // but a tool call is still running. The stream consumer checks this
    // and finishes the current tool call before breaking.
    let shutting_down = Arc::new(AtomicBool::new(false));
    let turn_active = Arc::new(AtomicBool::new(false));

    let mut rl = DefaultEditor::new()?;
    let history_path =
        dirs::home_dir().map(|h| h.join(".config/momo-fetch/history.txt"));

    if let Some(ref path) = history_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.load_history(path);
    }

    loop {
        // If shutting down signal was set by Ctrl+D during a turn, exit now
        if shutting_down.load(Ordering::Relaxed) && !turn_active.load(Ordering::Relaxed) {
            println!("Goodbye!");
            break;
        }

        let prompt = {
            let mode_tag = match harness.sandbox().permission_mode() {
                crate::sandbox::PermissionMode::Strict => String::new(),
                crate::sandbox::PermissionMode::Auto => " [auto]".to_string(),
                crate::sandbox::PermissionMode::Yolo => " [yolo]".to_string(),
            };
            format!(
                "{}{}> ",
                harness.provider_mgr().current_model_name().dimmed(),
                mode_tag.yellow(),
            )
        };
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
                        Ok(false) => {
                            // /quit — if a turn is somehow still active, wait gracefully
                            if turn_active.load(Ordering::Relaxed) {
                                shutting_down.store(true, Ordering::Relaxed);
                                // Wait for the active turn to complete
                                while turn_active.load(Ordering::Relaxed) {
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                }
                            }
                            println!("Goodbye!");
                            break;
                        }
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

                // Reset shutting_down for the new turn
                shutting_down.store(false, Ordering::Relaxed);

                // Run turn with streaming and graceful shutdown support
                run_turn_streaming(harness, &full_input, &shutting_down, &turn_active).await;

                // If shutting_down was set during the turn (e.g., Ctrl+C during
                // graceful shutdown), exit now
                if shutting_down.load(Ordering::Relaxed) {
                    println!("Goodbye!");
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C during input — cancel any active turn
                if turn_active.load(Ordering::Relaxed) {
                    shutting_down.store(true, Ordering::Relaxed);
                    harness.interrupt();
                    // Wait for the turn to gracefully complete
                    while turn_active.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    println!("Goodbye!");
                    break;
                }
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D — graceful shutdown
                if turn_active.load(Ordering::Relaxed) {
                    shutting_down.store(true, Ordering::Relaxed);
                    println!(
                        "\n{} Waiting for current operation to finish...",
                        "\u{23f3}".yellow()
                    );
                    // Wait for the active turn to complete gracefully
                    while turn_active.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
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

/// Prompt the user to approve or deny a tool call.
/// Returns Some(true) for approve, Some(false) for deny, None for no input.
fn prompt_tool_approval(tool_name: &str) -> Option<bool> {
    println!(
        "\n  {} Allow {} to proceed? {}",
        "?".yellow(),
        tool_name.yellow(),
        "[y/n]".dimmed()
    );
    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(_) => {
            let trimmed = answer.trim().to_lowercase();
            match trimmed.as_str() {
                "y" | "yes" => Some(true),
                "n" | "no" => Some(false),
                _ => None,
            }
        }
        Err(_) => None,
    }
}

/// Result of consuming a stream, including any pending tool confirmation.
struct StreamResult {
    has_output: bool,
    tool_calls: Vec<String>,
    response_parts: Vec<String>,
    pending_confirmation: Option<(String, String)>, // (tool_name, function_call_id)
}

impl Default for StreamResult {
    fn default() -> Self {
        Self {
            has_output: false,
            tool_calls: Vec::new(),
            response_parts: Vec::new(),
            pending_confirmation: None,
        }
    }
}

/// Run a single conversational turn with streaming output.
///
/// If auto_search is enabled, enriches the input with relevant memories.
/// If auto_write is enabled, writes a MemCell after the turn completes.
/// Handles tool confirmation prompts interactively.
async fn run_turn_streaming(
    harness: &mut Harness,
    input: &str,
    shutting_down: &Arc<AtomicBool>,
    turn_active: &Arc<AtomicBool>,
) {
    // Reset turn accumulators for cost tracking
    harness.cost_tracker().reset_turn();

    turn_active.store(true, Ordering::Relaxed);

    // Use enriched turn (auto-search prepends relevant memories)
    let enriched_result = harness.run_turn_enriched(input).await;
    let result = match enriched_result {
        Ok((_enriched_input, stream)) => {
            consume_stream(harness, stream, shutting_down).await
        }
        Err(e) => {
            println!("{} {}", "\u{2717}".red(), format!("{e}").red());
            StreamResult::default()
        }
    };

    if !result.has_output && result.pending_confirmation.is_none() {
        println!(
            "  {} No response from {} ({}). Check your API key and network.",
            "\u{26a0}".yellow(),
            harness.provider_mgr().current_provider(),
            harness.provider_mgr().current_model_name(),
        );
    }

    // Handle pending tool confirmation with interactive prompt
    if let Some((tool_name, _call_id)) = result.pending_confirmation {
        let tool_name_clone = tool_name.clone();
        let decision = prompt_tool_approval(&tool_name_clone);
        if let Some(approved) = decision {
            harness.cost_tracker().reset_turn();
            match harness.run_confirmation_turn(&tool_name_clone, approved).await {
                Ok(stream) => {
                    consume_stream(harness, stream, shutting_down).await;
                }
                Err(e) => {
                    println!("{} {}", "\u{2717}".red(), format!("{e}").red());
                }
            }
        } else {
            println!("  {} Tool call denied.", "\u{2717}".red());
        }
    }

    // Finalize cost tracking for this turn
    harness.cost_tracker().finalize_turn();

    // Check budget alerts
    if let Some(alert) = harness.cost_tracker().budget_alert() {
        println!("\n{}", alert.yellow());
    }

    // Post-turn: auto-write memory (Option A or B)
    if harness.memory_sidecar().auto_write_enabled() {
        let project_name = harness
            .config()
            .project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Collect turn summary from stream data and merge user_message
        let turn_summary = TURN_SUMMARY.with(|t| {
            let mut guard = t.borrow_mut();
            let mut summary = guard.take().unwrap_or_else(|| TurnSummary {
                user_message: String::new(),
                tool_calls: Vec::new(),
                response_preview: String::new(),
                project: project_name,
            });
            summary.user_message = input.to_string();
            summary
        });

        match harness.memory_sidecar().write_turn_memory_option_a(&turn_summary) {
            Ok(memcell_ref) => {
                tracing::debug!("Auto-wrote MemCell: {memcell_ref}");
            }
            Err(e) => {
                tracing::warn!("Auto-write MemCell failed: {e}");
            }
        }
    }

    turn_active.store(false, Ordering::Relaxed);
}

/// Consume an EventStream with colored output, Ctrl+C cancellation, and
/// graceful shutdown support.
///
/// Returns a StreamResult with collected data and any pending tool confirmation.
async fn consume_stream(
    harness: &mut Harness,
    mut stream: EventStream,
    shutting_down: &Arc<AtomicBool>,
) -> StreamResult {
    let mut result = StreamResult::default();
    let mut in_tool_call = false;
    let mut spinner = ThinkingSpinner::start();
    let mut event_count = 0usize;

    loop {
        tokio::select! {
            event_result = stream.next() => {
                match event_result {
                    Some(Ok(event)) => {
                        result.has_output = true;
                        event_count += 1;
                        spinner.stop();

                        // Capture usage metadata for cost tracking
                        if let Some(ref usage) = event.llm_response.usage_metadata {
                            harness.cost_tracker().record_event(usage);
                        }

                        // Handle tool confirmation request
                        if let Some(confirm_req) = &event.actions.tool_confirmation {
                            println!(
                                "\n  {} Tool {} requires approval: {}",
                                "!".yellow(),
                                confirm_req.tool_name.yellow(),
                                summarize_args(&confirm_req.args),
                            );
                            let call_id = confirm_req.function_call_id.clone().unwrap_or_default();
                            result.pending_confirmation = Some((confirm_req.tool_name.clone(), call_id));
                        }

                        // Display content parts
                        if let Some(content) = event.content() {
                            for part in &content.parts {
                                match part {
                                    Part::Text { text } => {
                                        print!("{}", text);
                                        let _ = std::io::stdout().flush();
                                        in_tool_call = false;
                                        result.response_parts.push(text.clone());
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
                                        result.tool_calls.push(format!("{}({})", name, summarize_args(args)));
                                    }
                                    Part::FunctionResponse { function_response, .. } => {
                                        if in_tool_call {
                                            let summary =
                                                summarize_response(&function_response.response);
                                            // Truncate long responses
                                            let truncated = if summary.len() > 200 {
                                                let end = summary.ceil_char_boundary(200);
                                                format!("{}...", &summary[..end])
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

                        // Restart spinner only if we just finished a tool call
                        // (LLM will think again to process the tool response).
                        // Don't restart if we just printed text — the LLM is
                        // still streaming and will send more content soon.
                        if in_tool_call {
                            spinner = ThinkingSpinner::start();
                        }

                        // Graceful shutdown: if not in a tool call, break
                        // (tool calls will be allowed to finish)
                        if shutting_down.load(Ordering::Relaxed) && !in_tool_call {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        spinner.stop();
                        println!("\n{} Stream error: {}", "\u{2717}".red(), e);
                        break;
                    }
                    None => {
                        spinner.stop();
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                if shutting_down.load(Ordering::Relaxed) {
                    // Second Ctrl+C during graceful shutdown — force exit
                    harness.interrupt();
                    println!("\n^C Force quit");
                    break;
                }
                // Normal Ctrl+C — cancel generation but allow partial response
                // to be saved in session (adk-runner handles session persistence)
                harness.interrupt();
                println!("\n^C Generation cancelled");
                break;
            }
        }
    }

    if result.has_output {
        println!(); // Trailing newline after response
    } else if result.pending_confirmation.is_none() {
        // No output and no confirmation — likely a connection or empty response issue
        tracing::warn!("Turn completed with no output");
    }

    // Store turn summary for post-turn memory write
    let response_preview: String = result.response_parts.join("");
    let preview = if response_preview.len() > 300 {
        let mut end = 300;
        while end > 0 && !response_preview.is_char_boundary(end) {
            end -= 1;
        }
        response_preview[..end].to_string()
    } else {
        response_preview
    };
    let project_name = harness
        .config()
        .project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    TURN_SUMMARY.with(|t| {
        *t.borrow_mut() = Some(TurnSummary {
            user_message: String::new(), // Filled in by run_turn_streaming
            tool_calls: result.tool_calls.clone(),
            response_preview: preview,
            project: project_name,
        });
    });

    result
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
    let s = response.to_string();
    // Remove outer quotes if it's a string
    if let serde_json::Value::String(inner) = response {
        inner.clone()
    } else {
        s
    }
}
