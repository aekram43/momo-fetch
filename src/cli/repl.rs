use std::io::Write;
use std::path::Path;
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

        let prompt = "you> ".to_string();
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
                    // For Unknown commands, try to resolve as a custom command first
                    if let super::commands::Command::Unknown(_) = cmd {
                        let working_dir = harness.sandbox().root();
                        if let Some(resolved_prompt) = try_custom_command(trimmed, working_dir) {
                            if resolved_prompt.trim().is_empty() {
                                println!(
                                    "  {} Custom command resolved to empty prompt.",
                                    "\u{26a0}".yellow()
                                );
                                continue;
                            }
                            run_turn_streaming(
                                harness,
                                &resolved_prompt,
                                &shutting_down,
                                &turn_active,
                            )
                            .await;
                            if shutting_down.load(Ordering::Relaxed) {
                                println!("Goodbye!");
                                break;
                            }
                            continue;
                        }
                        // Not a custom command — fall through to Unknown handler
                    }

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
    harness.begin_turn();

    turn_active.store(true, Ordering::Relaxed);

    // Show brief status — what we're about to do
    eprint!("\r\u{1b}[2K  {} Connecting to {}...", "\u{2192}".dimmed(), harness.provider_mgr().current_model_name().dimmed());
    let _ = std::io::stderr().flush();

    // Use enriched turn (auto-search prepends relevant memories)
    let enriched_result = harness.run_turn_enriched(input).await;
    let result = match enriched_result {
        Ok((_enriched_input, stream)) => {
            // Clear the "connecting" status line
            eprint!("\r\u{1b}[2K");
            let _ = std::io::stderr().flush();

            // Print agent response header (model + context usage)
            {
                let mode_tag = match harness.sandbox().permission_mode() {
                    crate::sandbox::PermissionMode::Strict => String::new(),
                    crate::sandbox::PermissionMode::Auto => " [auto]".to_string(),
                    crate::sandbox::PermissionMode::Yolo => " [yolo]".to_string(),
                };
                let context_tag = {
                    let tokens = harness.cost_tracker().last_prompt_tokens();
                    if tokens > 0 {
                        let provider = harness.provider_mgr().current_provider();
                        let model = harness.provider_mgr().current_model_name();
                        let usage = crate::context_window::ContextUsage::new_resolved(
                            tokens as i64, &provider, &model,
                            Some(&harness.config().context_window_overrides),
                            Some(harness.provider_mgr().context_window_cache().as_ref()),
                        );
                        format!(" \u{00b7} {}", usage.format_status())
                    } else {
                        String::new()
                    }
                };
                println!(
                    "{}{}{}",
                    harness.provider_mgr().current_model_name().bright_cyan(),
                    context_tag.dimmed(),
                    mode_tag.yellow(),
                );
                let _ = std::io::stdout().flush();
            }

            // Drain and display pending status events from background workers.
            // Shown here (after model header, before response) so they scroll
            // away naturally with the model's answer. Completed tasks show ✓,
            // active tasks show ⏳.
            {
                let events = harness.status_channel().drain_pending();
                if !events.is_empty() {
                    use crate::cli::status::format_inline;
                    let inline = format_inline(&events);
                    if !inline.is_empty() {
                        println!("{}", inline);
                    }
                }
            }

            consume_stream(harness, stream, shutting_down).await
        }
        Err(e) => {
            // Clear the "connecting" status line before showing error
            eprint!("\r\u{1b}[2K");
            let _ = std::io::stderr().flush();
            println!("{} {}", "\u{2717}".red(), format!("{e}").red());
            StreamResult::default()
        }
    };

    if result.response_parts.is_empty() && result.pending_confirmation.is_none() {
        if result.tool_calls.is_empty() {
            // No events at all — likely a connection or auth issue
            println!(
                "\n  {} No response from {} ({})\n  \
                 Possible causes:\n  \
                 - API key is invalid or expired\n  \
                 - Network connectivity issue\n  \
                 - Request exceeded context window (file too large?)\n  \
                 - Rate limit or quota exceeded\n  \
                 Try again or check your setup with /config",
                "\u{26a0}".yellow(),
                harness.provider_mgr().current_provider().dimmed(),
                harness.provider_mgr().current_model_name().dimmed(),
            );
        } else {
            // Tool calls executed but LLM didn't generate a text response
            let tool_count = result.tool_calls.len();
            println!(
                "\n  {} {} processed {} tool call(s) but didn't generate a response.\n  \
                 This usually means context window exceeded or response was filtered.\n  \
                 Try rephrasing or splitting the request into smaller parts.",
                "\u{26a0}".yellow(),
                harness.provider_mgr().current_model_name(),
                tool_count,
            );
        }
    }

    // Handle pending tool confirmation with interactive prompt
    if let Some((tool_name, _call_id)) = result.pending_confirmation {
        let tool_name_clone = tool_name.clone();
        let decision = prompt_tool_approval(&tool_name_clone);
        if let Some(approved) = decision {
            harness.begin_turn();
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
    harness.end_turn();

    // Check budget alerts
    if let Some(alert) = harness.cost_tracker().budget_alert() {
        println!("\n{}", alert.yellow());
    }

    // Check context window warnings
    {
        let tokens = harness.cost_tracker().last_prompt_tokens();
        if tokens > 0 {
            let provider = harness.provider_mgr().current_provider();
            let model = harness.provider_mgr().current_model_name();
            let usage = crate::context_window::ContextUsage::new_resolved(
                tokens as i64, &provider, &model,
                Some(&harness.config().context_window_overrides),
                Some(harness.provider_mgr().context_window_cache().as_ref()),
            );
            if let Some(warning) = usage.format_warning() {
                println!("\n{}", warning);
            }
        }
    }

    // Post-turn: auto-write memory (Option A or B) — run in background to avoid blocking the prompt
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

        // Clone what we need for the background task
        let sidecar = harness.memory_sidecar().clone_arc();
        let provider_mgr = harness.provider_mgr().clone();
        let mailbox_path = harness.mailbox_path();
        let status_sender = harness.status_channel().sender();

        std::thread::spawn(move || {
            match sidecar.write_turn_memory(
                &turn_summary,
                &provider_mgr,
                Some(&mailbox_path),
                Some(&status_sender),
            ) {
                Ok(memcell_ref) => {
                    tracing::debug!("Auto-wrote MemCell: {memcell_ref}");
                }
                Err(e) => {
                    tracing::warn!("Auto-write MemCell failed: {e}");
                }
            }
        });
    }

    // Note: Status events from background workers are NOT displayed here.
    // They are drained and displayed at the START of the next turn,
    // so they appear before the model's response and scroll away naturally.
    // This avoids permanent clutter between turns.

    turn_active.store(false, Ordering::Relaxed);
}

/// Maximum time to wait for the next stream event before timing out (120 seconds).
const STREAM_EVENT_TIMEOUT_SECS: u64 = 120;

/// "2m 30s" rather than "150s".
///
/// Once a wait is minutes long, seconds are the wrong unit to make someone
/// convert in their head while deciding whether to keep waiting.
fn fmt_duration(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let (m, s) = (secs / 60, secs % 60);
    if s == 0 {
        format!("{m}m")
    } else {
        format!("{m}m {s}s")
    }
}

/// Consume an EventStream with colored output, Ctrl+C cancellation,
/// stream timeout, and graceful shutdown support.
///
/// Returns a StreamResult with collected data and any pending tool confirmation.
async fn consume_stream(
    harness: &mut Harness,
    mut stream: EventStream,
    shutting_down: &Arc<AtomicBool>,
) -> StreamResult {
    let mut result = StreamResult::default();
    let mut in_tool_call = false;
    // Name of the tool currently running, so a long wait can say what for.
    let mut current_tool_label: Option<String> = None;
    let mut last_event_was_text = false;
    let mut had_tool_calls = false;
    let mut spinner = ThinkingSpinner::start();
    let mut consecutive_timeouts = 0u32;

    loop {
        tokio::select! {
            event_result = stream.next() => {
                consecutive_timeouts = 0; // Reset on any event
                match event_result {
                    Some(Ok(event)) => {
                        result.has_output = true;
                        spinner.stop();

                        // Capture usage metadata for cost tracking
                        if let Some(ref usage) = event.llm_response.usage_metadata {
                            harness.record_usage(usage);
                        }

                        // Handle tool confirmation request
                        if let Some(confirm_req) = &event.actions.tool_confirmation {
                            println!(
                                "\n  {} {} requires approval: {}",
                                "\u{26a0}".yellow(),
                                confirm_req.tool_name.cyan(),
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
                                        // Add separator when transitioning from tools to text
                                        if had_tool_calls && !last_event_was_text {
                                            println!();
                                        }
                                        print!("{}", text);
                                        let _ = std::io::stdout().flush();
                                        in_tool_call = false;
                                        last_event_was_text = true;
                                        result.response_parts.push(text.clone());
                                    }
                                    Part::FunctionCall { name, args, .. } => {
                                        println!(
                                            "  {} {}({})",
                                            "\u{25b8}".cyan(),
                                            name.cyan(),
                                            summarize_args(args),
                                        );
                                        in_tool_call = true;
                                        current_tool_label = Some(name.to_string());
                                        last_event_was_text = false;
                                        had_tool_calls = true;
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
                                                "    {} {}",
                                                "\u{21b3}".dimmed(),
                                                truncated.dimmed(),
                                            );
                                            in_tool_call = false;
                                            current_tool_label = None;
                                            last_event_was_text = false;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Check for errors in the response
                        if let Some(ref err) = event.llm_response.error_message {
                            if !err.is_empty() {
                                println!("\n  {} {}", "\u{2717}".red().bold(), err.red());
                            }
                        }

                        // Check if turn is complete
                        if event.is_final_response() {
                            break;
                        }

                        // Restart spinner after tool events (LLM is thinking about
                        // the next step). Don't restart after text — the LLM is
                        // still streaming and will send more content soon.
                        if !last_event_was_text {
                            // Show any status events from background workers
                            // (e.g. memory write progress) in the spinner area
                            let bg_events = harness.status_channel().drain_pending();
                            if !bg_events.is_empty() {
                                spinner.stop();
                                use crate::cli::status::format_inline;
                                let inline = format_inline(&bg_events);
                                if !inline.is_empty() {
                                    eprintln!("\r\u{1b}[K{}", inline);
                                }
                            }
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
            _ = tokio::time::sleep(std::time::Duration::from_secs(STREAM_EVENT_TIMEOUT_SECS)) => {
                consecutive_timeouts += 1;
                spinner.stop();
                let quiet_for = STREAM_EVENT_TIMEOUT_SECS * u64::from(consecutive_timeouts);

                // **A quiet stream is not a failure, and this no longer cancels.**
                //
                // It used to: two silent windows and the turn was killed with
                // "API timeout … Generation cancelled". That was wrong twice
                // over. A `task(...)` sub-agent runs entirely inside one tool
                // call and emits nothing to this stream while it works, so a
                // sub-agent given `timeout_secs=600` could never reach its own
                // deadline — this fired at 240s and killed it. And even when
                // nothing was running, a slow model was reported as a warning,
                // which reads as broken rather than busy.
                //
                // So: say what is happening and keep going. The tool's own
                // timeout still bounds its work, and Ctrl+C is the way out —
                // one control, held by the person watching, rather than a
                // guess made on their behalf.
                let what = if in_tool_call {
                    let tool = current_tool_label.as_deref().unwrap_or("a tool");
                    // "still working" is also what a hung process says. When a
                    // sub-agent is behind the call it can do better: name the
                    // subtask and the tool-call count, which move between two
                    // of these lines if anything is actually happening.
                    let runs = crate::tools::task::active_sub_agents();
                    match runs.first() {
                        Some(run) => format!(
                            "{tool} — {} ({} in)",
                            run.label(),
                            fmt_duration(run.elapsed_secs),
                        ),
                        None => format!("{tool} is still working"),
                    }
                } else {
                    format!(
                        "still generating on {}",
                        harness.provider_mgr().current_model_name(),
                    )
                };
                println!(
                    "  {} running in the background — {} ({} so far). Ctrl+C to stop.",
                    "\u{23f3}".cyan(),
                    what,
                    fmt_duration(quiet_for),
                );
                spinner = ThinkingSpinner::start();
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

    if !result.response_parts.is_empty() {
        println!(); // Trailing newline after text response
    } else if result.pending_confirmation.is_none() {
        // No text response — distinguish between no events at all vs
        // tool calls executed but LLM didn't generate text
        tracing::warn!("Turn completed with no text output");
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

// ─── Custom slash command resolution ───────────────────────────

/// Built-in command names that should never be checked against `.harness/commands/`.
const BUILTIN_COMMANDS: &[&str] = &[
    "help", "quit", "exit", "model", "provider", "models",
    "sessions", "resume", "cost", "mem", "kms", "skill",
    "mcp", "key", "agent", "team", "routine", "routines", "permission", "perm",
    "clear", "compact",
];

/// Try to resolve a slash command input as a user-defined custom command.
///
/// Looks for `.harness/commands/{name}.md` relative to `working_dir`.
/// If found, reads the file and replaces `$ARG` with the trailing text.
///
/// Returns `Some(resolved_prompt)` if a matching file exists, `None` otherwise.
pub(crate) fn try_custom_command(input: &str, working_dir: &Path) -> Option<String> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }

    let after_slash = &input[1..];
    let (name, arg) = match after_slash.find(' ') {
        Some(pos) => (&after_slash[..pos], after_slash[pos + 1..].trim()),
        None => (after_slash, ""),
    };

    if name.is_empty() {
        return None;
    }

    // Never intercept built-in commands
    if BUILTIN_COMMANDS.contains(&name) {
        return None;
    }

    // Only allow safe command names (alphanumeric, dash, underscore)
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }

    let cmd_file = working_dir
        .join(".harness")
        .join("commands")
        .join(format!("{name}.md"));

    if !cmd_file.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(&cmd_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{} Failed to read custom command '{name}': {e}",
                "\u{2717}".red()
            );
            return None;
        }
    };

    let resolved = if arg.is_empty() {
        content.replace("$ARG", "")
    } else {
        content.replace("$ARG", arg)
    };

    Some(resolved)
}

/// List available custom commands in `.harness/commands/`.
///
/// Returns a sorted list of `(name, description)` tuples where the description
/// is extracted from the first non-empty, non-comment line of the markdown file.
pub(crate) fn list_custom_commands(working_dir: &Path) -> Vec<(String, String)> {
    let commands_dir = working_dir.join(".harness").join("commands");
    if !commands_dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&commands_dir) else {
        return Vec::new();
    };

    let mut commands: Vec<(String, String)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? != "md" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let content = std::fs::read_to_string(&path).ok()?;
            let description = content
                .lines()
                .find(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !trimmed.starts_with("<!--")
                })
                .unwrap_or("(custom command)")
                .trim()
                .to_string();
            let desc = if description.len() > 60 {
                let end = description.ceil_char_boundary(57);
                format!("{}...", &description[..end])
            } else {
                description
            };
            Some((name, desc))
        })
        .collect();

    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_try_custom_command_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("review.md"), "Review this code: $ARG").unwrap();

        let result = try_custom_command("/review src/main.rs", tmp.path());
        assert_eq!(
            result,
            Some("Review this code: src/main.rs".to_string())
        );
    }

    #[test]
    fn test_try_custom_command_no_arg() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("review.md"), "Review this code: $ARG").unwrap();

        let result = try_custom_command("/review", tmp.path());
        assert_eq!(result, Some("Review this code: ".to_string()));
    }

    #[test]
    fn test_try_custom_command_no_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("status.md"), "Summarize current project status").unwrap();

        let result = try_custom_command("/status", tmp.path());
        assert_eq!(
            result,
            Some("Summarize current project status".to_string())
        );
    }

    #[test]
    fn test_try_custom_command_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = try_custom_command("/nonexistent", tmp.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_try_custom_command_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = try_custom_command("/anything", tmp.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_try_custom_command_builtin_bypass() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("help.md"), "This should not be used").unwrap();

        let result = try_custom_command("/help", tmp.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_try_custom_command_not_slash() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(try_custom_command("hello", tmp.path()), None);
    }

    #[test]
    fn test_try_custom_command_invalid_name() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("bad cmd.md"), "content").unwrap();

        assert_eq!(try_custom_command("/bad cmd", tmp.path()), None);
        assert_eq!(try_custom_command("/bad/cmd", tmp.path()), None);
    }

    #[test]
    fn test_list_custom_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let commands_dir = tmp.path().join(".harness").join("commands");
        fs::create_dir_all(&commands_dir).unwrap();
        fs::write(commands_dir.join("review.md"), "Review code for bugs").unwrap();
        fs::write(commands_dir.join("test.md"), "Write tests for: $ARG").unwrap();
        fs::write(
            commands_dir.join("deploy.md"),
            "<!-- deploy command -->\nDeploy the project",
        )
        .unwrap();

        let cmds = list_custom_commands(tmp.path());
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].0, "deploy");
        assert_eq!(cmds[1].0, "review");
        assert_eq!(cmds[2].0, "test");
    }

    #[test]
    fn test_list_custom_commands_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let cmds = list_custom_commands(tmp.path());
        assert!(cmds.is_empty());
    }
}

#[cfg(test)]
mod wait_tests {
    use super::fmt_duration;

    #[test]
    fn seconds_below_a_minute() {
        assert_eq!(fmt_duration(0), "0s");
        assert_eq!(fmt_duration(59), "59s");
    }

    #[test]
    fn whole_minutes_drop_the_seconds() {
        assert_eq!(fmt_duration(60), "1m");
        assert_eq!(fmt_duration(240), "4m");
    }

    #[test]
    fn minutes_and_seconds() {
        assert_eq!(fmt_duration(90), "1m 30s");
        assert_eq!(fmt_duration(605), "10m 5s");
    }
}
