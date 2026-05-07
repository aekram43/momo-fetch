use std::path::{Path, PathBuf};

use crate::memory::parser;
use crate::memory::types::{MemoryQuery, MemoryResult};
use crate::memory::vault::ObsidianVault;

/// Retrieve memories using grep + keyword relevance scoring.
///
/// Strategy: grep for query terms across vault files, then score candidates
/// using a TF-IDF-like approach (term frequency × inverse document frequency).
/// The "LLM" part of the name indicates this mode is designed for LLM-assisted
/// ranking, which can be layered on top when provider access is available.
pub fn grep_llm(vault: &ObsidianVault, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    let terms = tokenize(&query.query);
    if terms.is_empty() {
        return Ok(vec![]);
    }

    // Collect candidate files from specified levels
    let candidates = collect_candidates(vault, &query.levels, &query.project)?;

    // Score each candidate against the query terms
    let mut scored: Vec<MemoryResult> = candidates
        .into_iter()
        .filter_map(|(path, level, content)| {
            let score = compute_relevance(&terms, &content);
            if score > 0.0 {
                Some(MemoryResult {
                    ref_id: path_to_ref_id(&path, &level),
                    level,
                    relevance_score: score,
                    snippet: make_snippet(&content, &terms),
                    path,
                })
            } else {
                None
            }
        })
        .collect();

    // Sort by relevance descending
    scored.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));

    // Apply limit
    scored.truncate(query.limit);

    Ok(scored)
}

/// Retrieve memories by walking [[wikilinks]] from a seed note.
///
/// Starting from the note referenced in the query, follows outgoing wikilinks
/// to discover connected notes. Performs a breadth-first traversal up to a
/// configurable depth (default: 3 hops).
pub fn graph_walk(vault: &ObsidianVault, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    let seed_id = query.query.trim().to_string();
    if seed_id.is_empty() {
        return Ok(vec![]);
    }

    let max_depth = 3;
    let mut results: Vec<MemoryResult> = Vec::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut frontier: Vec<(String, usize)> = vec![(seed_id.clone(), 0)];
    visited.insert(seed_id.clone());

    // If the seed note itself exists, add it first
    if let Some((path, content)) = vault.read_note(&seed_id)? {
        let level = detect_level(&path);
        results.push(MemoryResult {
            ref_id: seed_id.clone(),
            level: level.clone(),
            relevance_score: 1.0,
            snippet: make_snippet(&content, &[]),
            path,
        });
    }

    while let Some((note_id, depth)) = frontier.pop() {
        if depth >= max_depth {
            continue;
        }

        // Find the note content to extract outgoing links
        let content = if let Some((_, c)) = vault.read_note(&note_id)? {
            c
        } else {
            // Try finding the note by searching for backlinks or daily log
            find_note_content(vault, &note_id)?
        };

        let links = parser::extract_wikilinks(&content);

        for link in links {
            let clean_id = link.split('#').next().unwrap_or(&link).to_string();
            if visited.contains(&clean_id) {
                continue;
            }
            visited.insert(clean_id.clone());

            if let Some((path, link_content)) = vault.read_note(&clean_id)? {
                let level = detect_level(&path);
                let score = 1.0 - (depth as f64 * 0.2); // Decay with depth
                results.push(MemoryResult {
                    ref_id: clean_id.clone(),
                    level,
                    relevance_score: score.max(0.2),
                    snippet: make_snippet(&link_content, &[]),
                    path,
                });
                frontier.push((clean_id, depth + 1));
            }
        }
    }

    // Sort by relevance and apply limit
    results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(query.limit);

    Ok(results)
}

/// Retrieve memories by filtering on YAML frontmatter tags.
///
/// Scans all notes in specified levels and returns those whose frontmatter
/// tags match any of the requested tags.
pub fn tag_filter(vault: &ObsidianVault, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    let required_tags = query.tags.as_deref().unwrap_or(&[]);
    if required_tags.is_empty() {
        // If no tags specified, return empty — caller should use grep_llm instead
        return Ok(vec![]);
    }

    let candidates = collect_candidates(vault, &query.levels, &query.project)?;

    let mut results: Vec<MemoryResult> = candidates
        .into_iter()
        .filter_map(|(path, level, content)| {
            let (fm, _) = parser::parse_frontmatter(&content);
            let note_tags = fm
                .as_ref()
                .map(|v| parser::parse_tags(v))
                .unwrap_or_default();

            // Count how many required tags match
            let match_count = required_tags
                .iter()
                .filter(|rt| note_tags.iter().any(|nt| nt.eq_ignore_ascii_case(rt)))
                .count();

            if match_count > 0 {
                let score = match_count as f64 / required_tags.len() as f64;
                Some(MemoryResult {
                    ref_id: path_to_ref_id(&path, &level),
                    level,
                    relevance_score: score,
                    snippet: make_snippet(&content, &[]),
                    path,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(query.limit);

    Ok(results)
}

/// Agentic retrieval: multi-round grep with progressive refinement.
///
/// Performs multiple rounds of searching:
/// 1. Initial broad grep using the query
/// 2. Extract key terms from top results
/// 3. Refined grep using extracted terms
/// 4. Merge and deduplicate results
///
/// This simulates what an LLM-driven agentic search would do, using
/// keyword extraction instead of LLM calls.
pub fn agentic(vault: &ObsidianVault, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
    let terms = tokenize(&query.query);
    if terms.is_empty() {
        return Ok(vec![]);
    }

    // Round 1: Initial broad search
    let round1_limit = query.limit * 2; // Get more candidates for refinement
    let round1_query = MemoryQuery {
        limit: round1_limit,
        ..query.clone()
    };
    let round1_results = grep_llm(vault, &round1_query)?;

    if round1_results.is_empty() {
        return Ok(vec![]);
    }

    // Extract expanded terms from top results
    let expanded_terms = extract_terms_from_results(&round1_results, &terms);

    // Round 2: Refined search using expanded terms
    let refined_query_str = format!(
        "{} {}",
        query.query,
        expanded_terms.join(" ")
    );
    let round2_query = MemoryQuery {
        query: refined_query_str,
        limit: query.limit,
        ..query.clone()
    };
    let round2_results = grep_llm(vault, &round2_query)?;

    // Merge: deduplicate by ref_id, keeping higher score
    let mut merged: std::collections::HashMap<String, MemoryResult> =
        std::collections::HashMap::new();

    for result in round1_results.into_iter().chain(round2_results.into_iter()) {
        let entry = merged.entry(result.ref_id.clone()).or_insert_with(|| {
            MemoryResult {
                ref_id: result.ref_id.clone(),
                level: result.level.clone(),
                relevance_score: 0.0,
                snippet: result.snippet.clone(),
                path: result.path.clone(),
            }
        });
        // Keep the higher score, boost merged results slightly
        let boosted = result.relevance_score * 1.1;
        if boosted > entry.relevance_score {
            entry.relevance_score = boosted.min(1.0);
            entry.snippet = result.snippet;
        }
    }

    let mut results: Vec<MemoryResult> = merged.into_values().collect();
    results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(query.limit);

    Ok(results)
}

// ─── Helper Functions ────────────────────────────────────────────────

/// Tokenize a query string into lowercase terms, filtering stop words.
fn tokenize(query: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "shall", "can", "need", "dare", "ought",
        "used", "to", "of", "in", "for", "on", "with", "at", "by", "from",
        "as", "into", "through", "during", "before", "after", "above", "below",
        "between", "out", "off", "over", "under", "again", "further", "then",
        "once", "and", "but", "or", "nor", "not", "so", "yet", "both", "either",
        "neither", "each", "every", "all", "any", "few", "more", "most", "other",
        "some", "such", "no", "only", "own", "same", "than", "too", "very",
        "just", "because", "if", "when", "where", "how", "what", "which", "who",
        "this", "that", "these", "those", "i", "me", "my", "we", "our", "you",
        "your", "he", "him", "his", "she", "her", "it", "its", "they", "them",
    ];

    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|s| !s.is_empty() && s.len() > 1)
        .filter(|s| !STOP_WORDS.contains(&s.as_ref()))
        .map(String::from)
        .collect()
}

/// Collect candidate files from the vault matching level/project filters.
fn collect_candidates(
    vault: &ObsidianVault,
    levels: &Option<Vec<String>>,
    project: &Option<String>,
) -> anyhow::Result<Vec<(PathBuf, String, String)>> {
    let vault_path = vault.path();

    // Determine which level directories to scan
    let all_levels = [
        ("1-memcells", "memcell"),
        ("2-events", "event"),
        ("3-foresights", "foresight"),
        ("4-episodes", "episode"),
        ("5-profile", "profile"),
        ("clusters", "cluster"),
    ];

    let target_levels: Vec<(&str, &str)> = if let Some(lvl) = levels {
        all_levels
            .iter()
            .filter(|(_, name)| lvl.iter().any(|l| l.eq_ignore_ascii_case(name)))
            .map(|(dir, name)| (*dir, *name))
            .collect()
    } else {
        all_levels.to_vec()
    };

    let mut candidates = Vec::new();

    for (dir_name, level_name) in &target_levels {
        let dir = vault_path.join(dir_name);
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|ext: &std::ffi::OsStr| ext == "md")
            })
        {
            let path = entry.path().to_path_buf();
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Filter by project if specified
                if let Some(proj) = project {
                    let (fm, _) = parser::parse_frontmatter(&content);
                    let note_project = fm
                        .as_ref()
                        .and_then(|v| v.get("project"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !note_project.eq_ignore_ascii_case(proj) {
                        continue;
                    }
                }

                candidates.push((path, level_name.to_string(), content));
            }
        }
    }

    Ok(candidates)
}

/// Compute relevance score for content against query terms.
///
/// Uses a simplified TF-IDF-like approach:
/// - Term frequency: how many query terms appear and how often
/// - Position bonus: terms appearing earlier in the document score higher
/// - Heading bonus: terms in headings (# lines) score higher
fn compute_relevance(terms: &[String], content: &str) -> f64 {
    if terms.is_empty() || content.is_empty() {
        return 0.0;
    }

    let content_lower = content.to_lowercase();
    let total_chars = content_lower.len().max(1) as f64;

    let mut score = 0.0;
    let mut matched_terms = 0;

    for term in terms {
        let term_lower = term.to_lowercase();
        let matches: Vec<usize> = content_lower
            .match_indices(&term_lower)
            .map(|(idx, _)| idx)
            .collect();

        if matches.is_empty() {
            continue;
        }

        matched_terms += 1;

        // Term frequency (normalized)
        let tf = matches.len() as f64;

        // Position bonus: earlier matches score higher
        let first_pos = matches[0] as f64 / total_chars;
        let position_bonus = 1.0 - (first_pos * 0.3);

        // Check if term appears in a heading line
        let heading_bonus = content
            .lines()
            .take(20) // Only check first 20 lines for headings
            .any(|line| {
                line.trim().starts_with('#') && line.to_lowercase().contains(&term_lower)
            });

        let heading_mult = if heading_bonus { 1.5 } else { 1.0 };

        score += tf * position_bonus * heading_mult;
    }

    // Coverage: how many query terms matched (0-1)
    let coverage = matched_terms as f64 / terms.len() as f64;

    // Normalize: coverage × raw score (capped at 1.0)
    let normalized = (coverage * (1.0 + score * 0.1)).min(1.0);

    normalized
}

/// Extract the reference ID from a file path and level.
fn path_to_ref_id(path: &Path, _level: &str) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Detect the memory level from a file path.
pub fn detect_level(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    if path_str.contains("1-memcells") {
        "memcell".to_string()
    } else if path_str.contains("2-events") {
        "event".to_string()
    } else if path_str.contains("3-foresights") {
        "foresight".to_string()
    } else if path_str.contains("4-episodes") {
        "episode".to_string()
    } else if path_str.contains("5-profile") {
        "profile".to_string()
    } else if path_str.contains("6-reflections") {
        "reflection".to_string()
    } else if path_str.contains("clusters") {
        "cluster".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Create a snippet from content, highlighting matching terms.
pub fn make_snippet(content: &str, _terms: &[String]) -> String {
    // Take the first meaningful lines (skip frontmatter)
    let (_, body) = parser::parse_frontmatter(content);

    let lines: Vec<&str> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();

    let snippet = lines.join(" ");
    if snippet.len() > 300 {
        // Find a safe UTF-8 boundary
        let mut end = 300;
        while end > 0 && !snippet.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &snippet[..end])
    } else {
        snippet
    }
}

/// Find note content by searching the vault for a note ID or wikilink target.
fn find_note_content(vault: &ObsidianVault, note_id: &str) -> anyhow::Result<String> {
    // Try reading directly first
    if let Some((_, content)) = vault.read_note(note_id)? {
        return Ok(content);
    }

    // Search for the note ID in daily logs (for memcell references like "2026-05-07")
    let date_part = note_id.split('#').next().unwrap_or(note_id);
    if date_id_pattern().is_match(date_part) {
        let parts: Vec<&str> = date_part.split('-').collect();
        if parts.len() == 3 {
            let memcell_path = vault
                .path()
                .join("1-memcells")
                .join(parts[0])
                .join(parts[1])
                .join(format!("{date_part}.md"));
            if memcell_path.exists() {
                return Ok(std::fs::read_to_string(&memcell_path)?);
            }
        }
    }

    Ok(String::new())
}

/// Regex for date-based IDs (YYYY-MM-DD format).
fn date_id_pattern() -> regex::Regex {
    regex::Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap()
}

/// Extract additional key terms from search results for query expansion.
fn extract_terms_from_results(results: &[MemoryResult], original_terms: &[String]) -> Vec<String> {
    let original_set: std::collections::HashSet<String> = original_terms
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    let mut term_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    // Take top 5 results for term extraction
    for result in results.iter().take(5) {
        let terms = tokenize(&result.snippet);
        for term in terms {
            if !original_set.contains(&term) {
                *term_counts.entry(term).or_insert(0) += 1;
            }
        }
    }

    // Return top 3 new terms by frequency
    let mut sorted: Vec<(String, usize)> = term_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    sorted.into_iter().take(3).map(|(t, _)| t).collect()
}

/// Find all notes that link TO the given note ID (backlinks).
pub fn find_backlinks(vault: &ObsidianVault, note_id: &str) -> anyhow::Result<Vec<MemoryResult>> {
    let vault_path = vault.path();
    let wikilink = format!("[[{note_id}]]");
    let wikilink_lower = wikilink.to_lowercase();

    let all_dirs = [
        "1-memcells",
        "2-events",
        "3-foresights",
        "4-episodes",
        "5-profile",
        "6-reflections",
        "clusters",
    ];

    let mut results = Vec::new();

    for dir_name in &all_dirs {
        let dir = vault_path.join(dir_name);
        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|ext: &std::ffi::OsStr| ext == "md")
            })
        {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if content.to_lowercase().contains(&wikilink_lower) {
                    let path = entry.path().to_path_buf();
                    let level = detect_level(&path);
                    results.push(MemoryResult {
                        ref_id: path_to_ref_id(&path, &level),
                        level,
                        relevance_score: 0.8, // Backlinks have high relevance
                        snippet: make_snippet(&content, &[]),
                        path,
                    });
                }
            }
        }
    }

    Ok(results)
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{ActionRecord, RetrievalMode};

    fn setup_vault_with_data() -> (tempfile::TempDir, ObsidianVault) {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Write and extract some data
        let memcell_ref = vault
            .write_memcell(
                "test-project",
                "Rust async patterns",
                "Learning about tokio and async runtime",
                &[ActionRecord {
                    description: "wrote async code".into(),
                    result: "compiles correctly".into(),
                }],
                "Async patterns work well",
                &["rust", "async", "tokio"],
            )
            .unwrap();

        vault
            .extract_from_memcell(
                &memcell_ref,
                "test-project",
                "Rust async patterns",
                "Learning about tokio and async runtime",
                &[ActionRecord {
                    description: "wrote async code".into(),
                    result: "compiles correctly".into(),
                }],
                "Async patterns work well",
                &["rust", "async", "tokio"],
            )
            .unwrap();

        // Second memcell about a different topic
        let memcell_ref2 = vault
            .write_memcell(
                "other-project",
                "Database design",
                "Designing PostgreSQL schema for users",
                &[],
                "Schema created",
                &["database", "postgresql", "schema"],
            )
            .unwrap();

        vault
            .extract_from_memcell(
                &memcell_ref2,
                "other-project",
                "Database design",
                "Designing PostgreSQL schema for users",
                &[],
                "Schema created",
                &["database", "postgresql", "schema"],
            )
            .unwrap();

        (tmp, vault)
    }

    #[test]
    fn test_tokenize() {
        let terms = tokenize("How do I use async patterns in Rust?");
        assert!(terms.contains(&"async".to_string()));
        assert!(terms.contains(&"patterns".to_string()));
        assert!(terms.contains(&"rust".to_string()));
        // Stop words should be filtered
        assert!(!terms.contains(&"how".to_string()));
        assert!(!terms.contains(&"do".to_string()));
        assert!(!terms.contains(&"i".to_string()));
    }

    #[test]
    fn test_tokenize_empty() {
        let terms = tokenize("");
        assert!(terms.is_empty());
    }

    #[test]
    fn test_grep_llm_finds_matching_notes() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: "rust async patterns".into(),
            mode: RetrievalMode::GrepLlm,
            levels: None,
            project: None,
            tags: None,
            limit: 10,
        };

        let results = grep_llm(&vault, &query).unwrap();
        assert!(!results.is_empty());

        // Should find notes about async/rust, not database
        let has_rust_result = results.iter().any(|r| {
            r.snippet.to_lowercase().contains("async")
                || r.snippet.to_lowercase().contains("rust")
        });
        assert!(has_rust_result);

        // All results should have positive relevance scores
        for r in &results {
            assert!(r.relevance_score > 0.0);
            assert!(r.relevance_score <= 1.0);
        }
    }

    #[test]
    fn test_grep_llm_project_filter() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: "design schema database".into(),
            mode: RetrievalMode::GrepLlm,
            levels: None,
            project: Some("other-project".into()),
            tags: None,
            limit: 10,
        };

        let results = grep_llm(&vault, &query).unwrap();
        // All results should be from other-project (about database, not rust/async)
        for r in &results {
            // Verify by reading the note's frontmatter project field
            let content = std::fs::read_to_string(&r.path).unwrap();
            let (fm, _) = parser::parse_frontmatter(&content);
            let proj = fm
                .as_ref()
                .and_then(|v| v.get("project"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            assert_eq!(proj, "other-project");
        }
    }

    #[test]
    fn test_grep_llm_level_filter() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: "rust".into(),
            mode: RetrievalMode::GrepLlm,
            levels: Some(vec!["event".into()]),
            project: None,
            tags: None,
            limit: 10,
        };

        let results = grep_llm(&vault, &query).unwrap();
        // All results should be events
        for r in &results {
            assert_eq!(r.level, "event");
        }
    }

    #[test]
    fn test_grep_llm_limit() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: "test".into(),
            mode: RetrievalMode::GrepLlm,
            levels: None,
            project: None,
            tags: None,
            limit: 2,
        };

        let results = grep_llm(&vault, &query).unwrap();
        assert!(results.len() <= 2);
    }

    #[test]
    fn test_tag_filter() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: String::new(), // Not used in tag_filter
            mode: RetrievalMode::TagFilter,
            levels: None,
            project: None,
            tags: Some(vec!["rust".into()]),
            limit: 10,
        };

        let results = tag_filter(&vault, &query).unwrap();
        assert!(!results.is_empty());

        // All results should have relevance > 0 (they match at least one tag)
        for r in &results {
            assert!(r.relevance_score > 0.0);
        }
    }

    #[test]
    fn test_tag_filter_no_tags() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: String::new(),
            mode: RetrievalMode::TagFilter,
            levels: None,
            project: None,
            tags: None,
            limit: 10,
        };

        let results = tag_filter(&vault, &query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_graph_walk_from_event() {
        let (_tmp, vault) = setup_vault_with_data();

        // Get an event ID from the vault
        let event_id = format!("fact-{:04}", vault.counters().event);

        let query = MemoryQuery {
            query: event_id.clone(),
            mode: RetrievalMode::GraphWalk,
            levels: None,
            project: None,
            tags: None,
            limit: 20,
        };

        let results = graph_walk(&vault, &query).unwrap();
        // Should find at least the seed note
        assert!(!results.is_empty());
        // First result should be the seed with highest relevance
        assert_eq!(results[0].ref_id, event_id);
        assert_eq!(results[0].relevance_score, 1.0);
    }

    #[test]
    fn test_graph_walk_empty_query() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: String::new(),
            mode: RetrievalMode::GraphWalk,
            levels: None,
            project: None,
            tags: None,
            limit: 10,
        };

        let results = graph_walk(&vault, &query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_agentic_multi_round() {
        let (_tmp, vault) = setup_vault_with_data();

        let query = MemoryQuery {
            query: "rust async tokio patterns".into(),
            mode: RetrievalMode::Agentic,
            levels: None,
            project: None,
            tags: None,
            limit: 10,
        };

        let results = agentic(&vault, &query).unwrap();
        assert!(!results.is_empty());

        // Results should be sorted by relevance
        for i in 1..results.len() {
            assert!(results[i].relevance_score <= results[i - 1].relevance_score);
        }
    }

    #[test]
    fn test_find_backlinks() {
        let (_tmp, vault) = setup_vault_with_data();

        // Get the first event's ID
        let event_id = format!("fact-{:04}", vault.counters().event);

        let backlinks = find_backlinks(&vault, &event_id).unwrap();
        // The episode should reference the event via wikilinks
        // (depends on extraction creating [[fact-XXXX]] links)
        // At minimum, the episode created during extraction should link back
        assert!(!backlinks.is_empty() || vault.stats().total_episodes == 0);
    }

    #[test]
    fn test_compute_relevance_basic() {
        let terms = vec!["rust".to_string(), "async".to_string()];
        let content = "This is about Rust async patterns in tokio runtime";
        let score = compute_relevance(&terms, content);
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_compute_relevance_no_match() {
        let terms = vec!["python".to_string(), "django".to_string()];
        let content = "This is about Rust async patterns in tokio runtime";
        let score = compute_relevance(&terms, content);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_make_snippet() {
        let content = "---\ntype: event\n---\n# My Title\n\nSome body text here";
        let snippet = make_snippet(content, &[]);
        assert!(snippet.contains("My Title"));
        assert!(!snippet.contains("type: event"));
    }
}
