use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// File read tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileReadArgs {
    /// File path relative to working directory
    pub path: String,
    /// Optional line range, e.g., "1-50"
    pub range: Option<String>,
}

/// File write tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileWriteArgs {
    /// File path relative to working directory
    pub path: String,
    /// Content to write
    pub content: String,
}

/// File edit tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileEditArgs {
    /// File path relative to working directory
    pub path: String,
    /// Exact string to find (must be unique in file)
    pub old_string: String,
    /// Replacement string
    pub new_string: String,
}

/// Read file contents with optional line range.
/// Returns content with line numbers (cat -n format).
pub async fn file_read(path: &str, range: Option<&str>) -> anyhow::Result<String> {
    let content = tokio::fs::read_to_string(path).await?;

    let result = if let Some(range) = range {
        let (start, end) = parse_line_range(range, content.lines().count())?;
        content
            .lines()
            .enumerate()
            .skip(start)
            .take(end - start + 1)
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content
            .lines()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(result)
}

/// Write content to a file. Creates parent directories if needed.
pub async fn file_write(path: &str, content: &str) -> anyhow::Result<()> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Atomic write: temp file → rename
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, &path).await?;

    Ok(())
}

/// Replace a unique string in a file with a new string.
/// Fails if old_string is not found or has multiple matches.
pub async fn file_edit(path: &str, old_string: &str, new_string: &str) -> anyhow::Result<usize> {
    let content = tokio::fs::read_to_string(path).await?;

    let matches = content.matches(old_string).count();
    if matches == 0 {
        anyhow::bail!("old_string not found in file");
    }
    if matches > 1 {
        anyhow::bail!(
            "old_string found {matches} times in file — must be unique. \
             Add more surrounding context to make it unique."
        );
    }

    let new_content = content.replacen(old_string, new_string, 1);

    // Atomic write
    let path = PathBuf::from(path);
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, &new_content).await?;
    tokio::fs::rename(&tmp, &path).await?;

    Ok(1)
}

/// Parse a line range string like "1-50" into (start, end) (0-indexed).
fn parse_line_range(range: &str, total_lines: usize) -> anyhow::Result<(usize, usize)> {
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid range format: '{range}'. Expected 'start-end' (e.g., '1-50')");
    }

    let start: usize = parts[0].trim().parse::<usize>()?.saturating_sub(1);
    let end: usize = parts[1].trim().parse::<usize>()?.min(total_lines);

    if start >= end {
        anyhow::bail!("Invalid range: start ({}) >= end ({})", start + 1, end);
    }

    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line_range() {
        assert_eq!(parse_line_range("1-50", 100).unwrap(), (0, 50));
        assert_eq!(parse_line_range("10-20", 100).unwrap(), (9, 20));
    }

    #[test]
    fn test_parse_line_range_invalid() {
        assert!(parse_line_range("50", 100).is_err());
        assert!(parse_line_range("abc-def", 100).is_err());
    }
}
