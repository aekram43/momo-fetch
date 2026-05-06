use serde::{Deserialize, Serialize};

/// Web search tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebSearchArgs {
    /// Search query
    pub query: String,
    /// Max results (default: 10)
    pub limit: Option<usize>,
}

/// Web fetch tool arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebFetchArgs {
    /// URL to fetch
    pub url: String,
    /// Response format: markdown or text
    pub format: Option<String>,
}

/// A single web search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Web search (placeholder — full implementation in US-005).
pub async fn web_search(query: &str, _limit: Option<usize>) -> anyhow::Result<Vec<WebSearchResult>> {
    // TODO: Implement via Serper.dev API or DuckDuckGo (US-005)
    Ok(vec![WebSearchResult {
        title: "Not yet implemented".into(),
        url: String::new(),
        snippet: format!("Search for '{query}' not yet implemented"),
    }])
}

/// Web fetch (placeholder — full implementation in US-005).
pub async fn web_fetch(url: &str, _format: Option<&str>) -> anyhow::Result<String> {
    let response = reqwest::get(url).await?;
    let content = response.text().await?;
    Ok(content)
}
