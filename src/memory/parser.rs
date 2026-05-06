// Memory vault parser utilities for frontmatter and wikilinks.

/// Parse YAML frontmatter from a markdown file.
/// Returns (frontmatter, body) tuple.
pub fn parse_frontmatter(content: &str) -> (Option<serde_yaml::Value>, &str) {
    if !content.starts_with("---") {
        return (None, content);
    }

    let rest = &content[3..];
    if let Some(end) = rest.find("---") {
        let frontmatter = &rest[..end];
        let body = &rest[end + 3..];
        let parsed = serde_yaml::from_str(frontmatter).ok();
        (parsed, body.trim_start_matches('\n'))
    } else {
        (None, content)
    }
}

/// Extract [[wikilinks]] from markdown content.
pub fn extract_wikilinks(content: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

/// Parse tags from YAML frontmatter.
pub fn parse_tags(frontmatter: &serde_yaml::Value) -> Vec<String> {
    frontmatter
        .get("tags")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\ntype: memcell\ndate: 2026-05-06\ntags: [test]\n---\nBody content";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_some());
        assert!(body.starts_with("Body content"));
    }

    #[test]
    fn test_parse_frontmatter_none() {
        let content = "Just body content";
        let (fm, body) = parse_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, "Just body content");
    }

    #[test]
    fn test_extract_wikilinks() {
        let content = "See [[2026-05-06#MemCell 001]] and [[some-note]] for details.";
        let links = extract_wikilinks(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "2026-05-06#MemCell 001");
        assert_eq!(links[1], "some-note");
    }
}
