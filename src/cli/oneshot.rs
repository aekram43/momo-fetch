use crate::harness::Harness;

/// Run a single prompt and exit.
pub async fn run(_harness: &Harness, prompt: &str) -> anyhow::Result<()> {
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

    // TODO: Send to agent via adk-runner (US-009)
    println!("(One-shot not yet implemented. Prompt: {full_prompt})");

    Ok(())
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
