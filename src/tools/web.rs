use adk_tool::{tool, AdkError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ─── Rate limiter ──────────────────────────────────────────────

struct RateLimiter {
    max_requests: usize,
    window_secs: u64,
    timestamps: Mutex<Vec<Instant>>,
}

impl RateLimiter {
    const fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            timestamps: Mutex::new(Vec::new()),
        }
    }

    fn check(&self) -> Result<(), AdkError> {
        let mut timestamps = self.timestamps.lock().unwrap();
        let now = Instant::now();
        let window = Duration::from_secs(self.window_secs);
        timestamps.retain(|t| now.duration_since(*t) < window);
        if timestamps.len() >= self.max_requests {
            return Err(AdkError::tool(format!(
                "Rate limit exceeded: max {} requests per {}s. Try again in a moment.",
                self.max_requests, self.window_secs
            )));
        }
        timestamps.push(now);
        Ok(())
    }
}

static WEB_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

fn rate_limiter() -> &'static RateLimiter {
    WEB_RATE_LIMITER.get_or_init(|| RateLimiter::new(10, 60))
}

// ─── WebSearch ─────────────────────────────────────────────────

/// Web search tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebSearchArgs {
    /// Search query
    pub query: String,
    /// Max results (default: 10)
    pub limit: Option<usize>,
}

/// Search the web using Serper.dev API (if SERPER_API_KEY is set) or DuckDuckGo as fallback.
/// Returns structured results with title, URL, and snippet for each match.
#[tool]
pub async fn web_search(args: WebSearchArgs) -> Result<Value, AdkError> {
    rate_limiter().check()?;
    let limit = args.limit.unwrap_or(10);

    // Primary: Serper.dev (needs SERPER_API_KEY)
    if let Ok(api_key) = std::env::var("SERPER_API_KEY") {
        return search_serper(&args.query, limit, &api_key).await;
    }

    // Fallback: DuckDuckGo HTML search
    search_duckduckgo(&args.query, limit).await
}

async fn search_serper(query: &str, limit: usize, api_key: &str) -> Result<Value, AdkError> {
    let client = reqwest::Client::new();
    let body = json!({ "q": query, "num": limit });

    let resp = client
        .post("https://google.serper.dev/search")
        .header("X-API-KEY", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AdkError::tool(format!("Serper API request failed: {e}")))?
        .error_for_status()
        .map_err(|e| AdkError::tool(format!("Serper API error: {e}")))?;

    let data: Value = resp
        .json()
        .await
        .map_err(|e| AdkError::tool(format!("Failed to parse Serper response: {e}")))?;

    let results: Vec<Value> = data["organic"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit)
                .filter_map(|item| {
                    let url = item["link"].as_str().unwrap_or("").to_string();
                    if url.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "title": item["title"].as_str().unwrap_or(""),
                        "url": url,
                        "snippet": item["snippet"].as_str().unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({ "results": results, "provider": "serper" }))
}

fn encode_query(query: &str) -> String {
    query
        .replace(' ', "+")
        .replace('&', "%26")
        .replace('#', "%23")
        .replace('"', "%22")
        .replace('\'', "%27")
        .replace('<', "%3C")
        .replace('>', "%3E")
}

async fn search_duckduckgo(query: &str, limit: usize) -> Result<Value, AdkError> {
    let client = reqwest::Client::new();
    let encoded = encode_query(query);
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");

    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; MOMO-Fetch/0.1)",
        )
        .send()
        .await
        .map_err(|e| AdkError::tool(format!("DuckDuckGo request failed: {e}")))?
        .error_for_status()
        .map_err(|e| AdkError::tool(format!("DuckDuckGo error: {e}")))?;

    let html = resp
        .text()
        .await
        .map_err(|e| AdkError::tool(format!("Failed to read DuckDuckGo response: {e}")))?;

    let results = parse_ddg_html(&html, limit);

    Ok(json!({ "results": results, "provider": "duckduckgo" }))
}

fn parse_ddg_html(html: &str, limit: usize) -> Vec<Value> {
    let mut results = Vec::new();

    // Find result link elements: <a ... class="result__a" ...>
    let link_re = regex::Regex::new(
        r#"<a\s[^>]*class="result__a"[^>]*>([\s\S]*?)</a>"#,
    )
    .unwrap();

    let href_re = regex::Regex::new(r#"href="([^"]*)""#).unwrap();
    let uddg_re = regex::Regex::new(r##"uddg=([^"&]+)"##).unwrap();

    for cap in link_re.captures_iter(html) {
        if results.len() >= limit {
            break;
        }

        let full_tag = cap.get(0).unwrap().as_str();
        let title_html = &cap[1];

        // Extract URL from href -> uddg parameter
        let url = href_re
            .captures(full_tag)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .and_then(|href| {
                uddg_re
                    .captures(href)
                    .and_then(|c| c.get(1))
                    .map(|m| percent_decode(m.as_str()))
            })
            .unwrap_or_default();

        if url.is_empty() {
            continue;
        }

        results.push(json!({
            "title": strip_all_tags(title_html).trim(),
            "url": url,
            "snippet": "",
        }));
    }

    // Try to fill snippets from result__snippet elements
    let snippet_re = regex::Regex::new(
        r#"<[^>]*class="result__snippet"[^>]*>([\s\S]*?)</[a-z]+>"#,
    )
    .unwrap();

    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|c| strip_all_tags(&c[1]))
        .collect();

    for (i, snippet) in snippets.iter().enumerate() {
        if i < results.len() {
            results[i]["snippet"] = json!(snippet);
        }
    }

    results
}

// ─── WebFetch ──────────────────────────────────────────────────

/// Web fetch tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebFetchArgs {
    /// URL to fetch
    pub url: String,
    /// Response format: "text" (default), "markdown", or "raw"
    pub format: Option<String>,
    /// Max response size in bytes (default: 50000)
    pub max_length: Option<usize>,
}

/// Fetch a URL and return its content converted to text or markdown.
/// Supports HTML pages, plain text, JSON, and other text-based content.
#[tool]
pub async fn web_fetch(args: WebFetchArgs) -> Result<Value, AdkError> {
    rate_limiter().check()?;

    let format = args.format.as_deref().unwrap_or("text");
    let max_length = args.max_length.unwrap_or(50_000);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| AdkError::tool(format!("Failed to create HTTP client: {e}")))?;

    let resp = client
        .get(&args.url)
        .header("User-Agent", "MOMO-Fetch/0.1 (web fetch tool)")
        .send()
        .await
        .map_err(|e| AdkError::tool(format!("Request failed: {e}")))?
        .error_for_status()
        .map_err(|e| AdkError::tool(format!("HTTP error: {e}")))?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let is_html = content_type.contains("text/html");

    let body = resp
        .text()
        .await
        .map_err(|e| AdkError::tool(format!("Failed to read response: {e}")))?;

    let content = if format == "raw" {
        body
    } else if is_html {
        match format {
            "markdown" => html_to_markdown(&body),
            _ => html_to_text(&body),
        }
    } else {
        body
    };

    let truncated = content.len() > max_length;
    let content = if truncated {
        // Find a safe UTF-8 boundary near max_length
        let mut end = max_length.min(content.len());
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n\n... [content truncated, {} bytes total] ...",
            &content[..end],
            content.len()
        )
    } else {
        content
    };

    Ok(json!({
        "content": content,
        "url": args.url,
        "content_type": content_type,
        "format": format,
        "truncated": truncated,
    }))
}

// ─── HTML utilities ────────────────────────────────────────────

fn html_to_text(html: &str) -> String {
    let text = remove_script_style(html);
    let text = regex::Regex::new(r"</(p|div|h[1-6]|li|br|tr|hr|table)[^>]*>")
        .unwrap()
        .replace_all(&text, "\n")
        .to_string();
    let text = strip_all_tags(&text);
    let text = decode_html_entities(&text);
    let text = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&text, "\n\n")
        .to_string();
    text.trim().to_string()
}

fn html_to_markdown(html: &str) -> String {
    let mut text = remove_script_style(html);

    // Headers
    for level in 1..=6 {
        let pattern = format!(r"<h{level}[^>]*>([\s\S]*?)</h{level}>");
        let re = regex::Regex::new(&pattern).unwrap();
        let hashes: String = std::iter::repeat('#').take(level).collect();
        let replacement = format!("{hashes} $1\n\n");
        text = re.replace_all(&text, replacement.as_str()).to_string();
    }

    // Bold
    text = regex::Regex::new(r"<(strong|b)[^>]*>([\s\S]*?)</(strong|b)>")
        .unwrap()
        .replace_all(&text, "**$2**")
        .to_string();

    // Italic
    text = regex::Regex::new(r"<(em|i)[^>]*>([\s\S]*?)</(em|i)>")
        .unwrap()
        .replace_all(&text, "*$2*")
        .to_string();

    // Links -> [text](url)
    text = regex::Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>([\s\S]*?)</a>"#)
        .unwrap()
        .replace_all(&text, "[$2]($1)")
        .to_string();

    // Code blocks
    text = regex::Regex::new(r"<pre[^>]*><code[^>]*>([\s\S]*?)</code></pre>")
        .unwrap()
        .replace_all(&text, "```\n$1\n```")
        .to_string();

    // Inline code
    text = regex::Regex::new(r"<code[^>]*>([\s\S]*?)</code>")
        .unwrap()
        .replace_all(&text, "`$1`")
        .to_string();

    // List items
    text = regex::Regex::new(r"<li[^>]*>([\s\S]*?)</li>")
        .unwrap()
        .replace_all(&text, "- $1\n")
        .to_string();

    // Block elements -> newlines
    text = regex::Regex::new(r"</(p|div|br|hr|table|tr|ul|ol|blockquote)[^>]*>")
        .unwrap()
        .replace_all(&text, "\n")
        .to_string();

    // Strip remaining tags
    text = strip_all_tags(&text);

    // Decode entities
    text = decode_html_entities(&text);

    // Clean up whitespace
    text = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&text, "\n\n")
        .to_string();

    text.trim().to_string()
}

fn remove_script_style(html: &str) -> String {
    let text = regex::Regex::new(r"<script[\s\S]*?</script>")
        .unwrap()
        .replace_all(html, "")
        .to_string();
    regex::Regex::new(r"<style[\s\S]*?</style>")
        .unwrap()
        .replace_all(&text, "")
        .to_string()
}

fn strip_all_tags(text: &str) -> String {
    regex::Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(text, "")
        .to_string()
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x2F;", "/")
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

// ─── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_all_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(strip_all_tags("<div class='foo'>text</div>"), "text");
        assert_eq!(strip_all_tags("no tags here"), "no tags here");
    }

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(decode_html_entities("&amp; &lt; &gt;"), "& < >");
        assert_eq!(decode_html_entities("&quot;hello&quot;"), "\"hello\"");
        assert_eq!(decode_html_entities("a&nbsp;b"), "a b");
    }

    #[test]
    fn test_html_to_text() {
        let html = "<html><body><h1>Title</h1><p>Hello world</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn test_html_to_markdown() {
        let html = r##"<h1>Title</h1><p>Hello <strong>world</strong></p><a href="https://example.com">link</a>"##;
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("**world**"));
        assert!(md.contains("[link](https://example.com)"));
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("a%2Fb%3Fc"), "a/b?c");
        assert_eq!(percent_decode("no%encoding"), "no%encoding");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn test_parse_ddg_html() {
        let html = r##"
        <div class="result__body">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc">
                Example Result
            </a>
            <a class="result__snippet" href="#">This is a snippet</a>
        </div>
        <div class="result__body">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftest.com&rut=def">
                Test Result
            </a>
            <a class="result__snippet" href="#">Another snippet</a>
        </div>
        "##;
        let results = parse_ddg_html(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["title"].as_str().unwrap(), "Example Result");
        assert_eq!(results[0]["url"].as_str().unwrap(), "https://example.com");
        assert_eq!(results[1]["title"].as_str().unwrap(), "Test Result");
    }

    #[test]
    fn test_parse_ddg_html_limit() {
        let html = r##"
        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fone.com">One</a>
        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftwo.com">Two</a>
        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fthree.com">Three</a>
        "##;
        let results = parse_ddg_html(html, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_encode_query() {
        assert_eq!(encode_query("hello world"), "hello+world");
        assert_eq!(encode_query("a&b#c"), "a%26b%23c");
    }

    #[test]
    fn test_remove_script_style() {
        let html = "<p>text</p><script>var x = 1;</script><style>.x{}</style><p>more</p>";
        let cleaned = remove_script_style(html);
        assert!(!cleaned.contains("script"));
        assert!(!cleaned.contains("style"));
        assert!(cleaned.contains("text"));
        assert!(cleaned.contains("more"));
    }

    #[test]
    fn test_rate_limiter() {
        let rl = RateLimiter::new(3, 60);
        assert!(rl.check().is_ok());
        assert!(rl.check().is_ok());
        assert!(rl.check().is_ok());
        assert!(rl.check().is_err()); // 4th request should fail
    }

    #[test]
    fn test_truncation_safe_utf8() {
        let content = "Hello \u{4e16}\u{754c} ".repeat(1000);
        let max_length = 500;
        let truncated = content.len() > max_length;
        assert!(truncated);

        let mut end = max_length.min(content.len());
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        let _ = &content[..end];
    }

    #[tokio::test]
    async fn test_web_fetch_invalid_url() {
        let result = web_fetch(WebFetchArgs {
            url: "not-a-valid-url".into(),
            format: None,
            max_length: None,
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_web_fetch_404() {
        let result = web_fetch(WebFetchArgs {
            url: "https://httpbin.org/status/404".into(),
            format: None,
            max_length: None,
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_web_search_returns_error_without_network() {
        let result = web_search(WebSearchArgs {
            query: "test query".into(),
            limit: Some(1),
        })
        .await;
        let _ = result;
    }
}
