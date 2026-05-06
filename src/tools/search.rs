use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Grep tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GrepArgs {
    /// Regex pattern to search for
    pub pattern: String,
    /// Directory or file to search in
    pub path: Option<String>,
    /// File glob filter
    pub glob: Option<String>,
}

/// Glob tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobArgs {
    /// Glob pattern to match files
    pub pattern: String,
    /// Directory to search in
    pub path: Option<String>,
}

/// Search file content using regex.
pub async fn grep(pattern: &str, path: Option<&str>) -> anyhow::Result<Vec<GrepMatch>> {
    let search_path = PathBuf::from(path.unwrap_or("."));
    let re = regex::Regex::new(pattern)?;

    let mut matches = Vec::new();

    if search_path.is_file() {
        search_file(&search_path, &re, &mut matches)?;
    } else if search_path.is_dir() {
        for entry in walkdir::WalkDir::new(&search_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                search_file(entry.path(), &re, &mut matches)?;
            }
        }
    }

    Ok(matches)
}

fn search_file(path: &std::path::Path, re: &regex::Regex, matches: &mut Vec<GrepMatch>) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path)?;
    for (line_num, line) in content.lines().enumerate() {
        if re.is_match(line) {
            matches.push(GrepMatch {
                path: path.display().to_string(),
                line_number: line_num + 1,
                line: line.to_string(),
            });
        }
    }
    Ok(())
}

/// Find files by glob pattern.
pub fn find_by_glob(pattern: &str, path: Option<&str>) -> anyhow::Result<Vec<String>> {
    let search_path = PathBuf::from(path.unwrap_or("."));
    let full_pattern = search_path.join(pattern);
    let pattern_str = full_pattern.display().to_string();

    let mut results = Vec::new();
    for entry in glob::glob(&pattern_str)?.filter_map(|e| e.ok()) {
        results.push(entry.display().to_string());
    }

    Ok(results)
}

/// A single grep match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}
