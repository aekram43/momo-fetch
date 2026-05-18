//! MCP (Model Context Protocol) integration using adk-tool's built-in MCP support.
//!
//! Supports both transport types:
//! - **stdio**: Local MCP servers spawned as child processes (`command` + `args`)
//! - **HTTP/SSE**: Remote MCP servers via streamable HTTP transport (`url` + `headers`)
//!
//! Config is loaded from `.harness/mcp.json` in Kiro-compatible format.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use adk_tool::mcp::manager::{McpServerConfig, McpServerManager, ServerStatus};
use adk_tool::mcp::AutoDeclineElicitationHandler;
use adk_tool::toolset::MergedToolset;
use adk_tool::Toolset;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── Config types ──────────────────────────────────────────────────────

/// MCP JSON config file format (Kiro-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpJsonFile {
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// HTTP-based MCP server config (Claude Code / Kiro compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpMcpServerConfig {
    /// Server type discriminator: "http" or "sse".
    #[serde(rename = "type")]
    pub server_type: String,
    /// MCP endpoint URL.
    pub url: String,
    /// Custom headers (e.g., Authorization).
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Whether this server is disabled.
    #[serde(default)]
    pub disabled: bool,
}

/// Parsed MCP config — separates stdio and HTTP servers.
struct ParsedMcpConfig {
    stdio_servers: HashMap<String, McpServerConfig>,
    http_servers: HashMap<String, HttpMcpServerConfig>,
    skipped: Vec<String>,
}

/// Parse an mcp.json file, separating stdio and HTTP servers.
fn parse_mcp_config(content: &str) -> Result<ParsedMcpConfig> {
    let raw: Value = serde_json::from_str(content)
        .map_err(|e| anyhow!("failed to parse MCP config as JSON: {e}"))?;

    let servers = match raw.get("mcpServers") {
        Some(Value::Object(map)) => map,
        _ => {
            return Ok(ParsedMcpConfig {
                stdio_servers: HashMap::new(),
                http_servers: HashMap::new(),
                skipped: Vec::new(),
            })
        }
    };

    let mut stdio_servers = HashMap::new();
    let mut http_servers = HashMap::new();
    let mut skipped = Vec::new();

    for (id, config_value) in servers {
        match config_value {
            Value::Object(obj) => {
                let server_type = obj.get("type").and_then(|v| v.as_str());

                if server_type == Some("http") || server_type == Some("sse") {
                    // HTTP/SSE server — parse as HTTP config
                    match serde_json::from_value::<HttpMcpServerConfig>(config_value.clone()) {
                        Ok(config) => {
                            if config.disabled {
                                tracing::info!("MCP: skipping disabled HTTP server '{id}'");
                                skipped.push(format!("{id} (disabled)"));
                            } else {
                                http_servers.insert(id.clone(), config);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Skipping HTTP MCP server '{}': invalid config: {e}", id);
                            skipped.push(format!("{id} (invalid HTTP config: {e})"));
                        }
                    }
                } else if obj.contains_key("command") || server_type == Some("stdio") || server_type.is_none() {
                    // Stdio server — parse normally
                    match serde_json::from_value::<McpServerConfig>(config_value.clone()) {
                        Ok(config) => {
                            stdio_servers.insert(id.clone(), config);
                        }
                        Err(e) => {
                            tracing::warn!("Skipping MCP server '{}': invalid config: {e}", id);
                            skipped.push(format!("{id} (invalid: {e})"));
                        }
                    }
                } else {
                    let t = server_type.unwrap_or("unknown");
                    tracing::warn!("Skipping MCP server '{}' (unknown type: '{}')", id, t);
                    skipped.push(format!("{id} (unknown type: {t})"));
                }
            }
            _ => {
                tracing::warn!("Skipping MCP server '{}': expected object, got {}", id, config_value);
                skipped.push(format!("{id} (invalid format)"));
            }
        }
    }

    Ok(ParsedMcpConfig {
        stdio_servers,
        http_servers,
        skipped,
    })
}

/// Connect to an HTTP MCP server using McpHttpClientBuilder.
async fn connect_http_server(
    id: &str,
    config: &HttpMcpServerConfig,
) -> Result<Arc<dyn Toolset>> {
    use adk_tool::mcp::{McpAuth, McpHttpClientBuilder};

    let mut builder = McpHttpClientBuilder::new(&config.url);

    // Extract auth from headers
    for (key, value) in &config.headers {
        if key.eq_ignore_ascii_case("authorization") {
            // Strip "Bearer " prefix if present — McpHttpClientBuilder adds it via auth_header()
            let token = value.trim().strip_prefix("Bearer ").unwrap_or(value);
            let auth = McpAuth::bearer(token);
            builder = builder.with_auth(auth);
        } else {
            builder = builder.header(key.as_str(), value.as_str());
        }
    }

    tracing::info!("MCP HTTP: connecting to '{id}' at {}", config.url);

    let toolset = builder
        .connect()
        .await
        .map_err(|e| anyhow!("failed to connect HTTP MCP server '{id}': {e}"))?;

    Ok(Arc::new(toolset))
}

// ─── McpManager (unused but kept for API compat) ──────────────────────

/// Manages MCP server connections for the harness (stdio-only wrapper).
pub struct McpManager {
    manager: Arc<McpServerManager>,
    config_path: PathBuf,
}

impl McpManager {
    pub fn new(project_path: &Path) -> Result<Self> {
        let config_path = project_path.join(".harness").join("mcp.json");

        let manager = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| anyhow!("failed to read MCP config: {e}"))?;
            let parsed = parse_mcp_config(&content)?;
            for skip in &parsed.skipped {
                tracing::info!("MCP: skipped {skip}");
            }
            McpServerManager::new(parsed.stdio_servers)
                .with_elicitation_handler(Arc::new(AutoDeclineElicitationHandler))
                .with_name("momo-fetch-mcp")
        } else {
            McpServerManager::new(HashMap::new())
                .with_elicitation_handler(Arc::new(AutoDeclineElicitationHandler))
                .with_name("momo-fetch-mcp")
        };

        Ok(Self {
            manager: Arc::new(manager),
            config_path,
        })
    }

    pub fn new_empty() -> Result<Self> {
        let manager = McpServerManager::new(HashMap::new()).with_name("momo-fetch-mcp");
        Ok(Self {
            manager: Arc::new(manager),
            config_path: PathBuf::new(),
        })
    }

    pub async fn start_all(&self) -> HashMap<String, Result<()>> {
        self.manager
            .start_all()
            .await
            .into_iter()
            .map(|(id, res)| (id, res.map_err(|e| anyhow!("{e}"))))
            .collect()
    }

    pub fn start_monitoring(&self) {
        self.manager.start_monitoring();
    }
    pub fn stop_monitoring(&self) {
        self.manager.stop_monitoring();
    }

    pub async fn add_server(&self, id: String, config: McpServerConfig) -> Result<()> {
        self.manager
            .add_server(id.clone(), config)
            .await
            .map_err(|e| anyhow!("failed to add MCP server '{id}': {e}"))?;
        Ok(())
    }

    pub async fn remove_server(&self, id: &str) -> Result<()> {
        self.manager
            .remove_server(id)
            .await
            .map_err(|e| anyhow!("failed to remove MCP server '{id}': {e}"))?;
        Ok(())
    }

    pub async fn start_server(&self, id: &str) -> Result<()> {
        self.manager
            .start_server(id)
            .await
            .map_err(|e| anyhow!("failed to start MCP server '{id}': {e}"))
    }

    pub async fn stop_server(&self, id: &str) -> Result<()> {
        self.manager
            .stop_server(id)
            .await
            .map_err(|e| anyhow!("failed to stop MCP server '{id}': {e}"))
    }

    pub async fn all_statuses(&self) -> HashMap<String, ServerStatus> {
        self.manager.all_statuses().await
    }

    pub async fn server_status(&self, id: &str) -> Result<ServerStatus> {
        self.manager
            .server_status(id)
            .await
            .map_err(|e| anyhow!("failed to get status for '{id}': {e}"))
    }

    pub async fn running_count(&self) -> usize {
        self.manager.running_server_count().await
    }

    pub fn manager(&self) -> Arc<McpServerManager> {
        self.manager.clone()
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.manager
            .shutdown()
            .await
            .map_err(|e| anyhow!("failed to shutdown MCP servers: {e}"))
    }
}

// ─── McpService (main service used by harness) ─────────────────────────

/// Manages MCP config, connections, and toolset composition.
///
/// Handles both stdio servers (via `McpServerManager`) and HTTP servers
/// (via `McpHttpClientBuilder`). Exposes a merged toolset for registration
/// with the LLM agent.
pub struct McpService {
    manager: Arc<McpServerManager>,
    configs: HashMap<String, McpServerConfig>,
    http_configs: HashMap<String, HttpMcpServerConfig>,
    http_toolsets: Vec<Arc<dyn Toolset>>,
    config_path: PathBuf,
}

impl McpService {
    /// Create a new McpService by loading config from `.harness/mcp.json`.
    pub fn new(project_path: &Path) -> Result<Self> {
        let harness_dir = project_path.join(".harness");
        let config_path = harness_dir.join("mcp.json");

        let (configs, http_configs) = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| anyhow!("failed to read MCP config: {e}"))?;
            let parsed = parse_mcp_config(&content)?;
            for skip in &parsed.skipped {
                tracing::info!("MCP: skipped {skip}");
            }
            (parsed.stdio_servers, parsed.http_servers)
        } else {
            (HashMap::new(), HashMap::new())
        };

        let manager = McpServerManager::new(configs.clone())
            .with_elicitation_handler(Arc::new(AutoDeclineElicitationHandler))
            .with_name("momo-fetch-mcp");

        Ok(Self {
            manager: Arc::new(manager),
            configs,
            http_configs,
            http_toolsets: Vec::new(),
            config_path,
        })
    }

    /// Create with no servers (for testing).
    pub fn new_empty() -> Result<Self> {
        let manager = McpServerManager::new(HashMap::new()).with_name("momo-fetch-mcp");
        Ok(Self {
            manager: Arc::new(manager),
            configs: HashMap::new(),
            http_configs: HashMap::new(),
            http_toolsets: Vec::new(),
            config_path: PathBuf::new(),
        })
    }

    /// Start all configured servers (stdio + HTTP).
    pub async fn start_all(&self) -> HashMap<String, Result<()>> {
        let results = self
            .manager
            .start_all()
            .await
            .into_iter()
            .map(|(id, res)| (id, res.map_err(|e| anyhow!("{e}"))))
            .collect::<HashMap<_, _>>();
        results
    }

    /// Connect to all HTTP MCP servers.
    /// Must be called after `new()` and before `toolset()`.
    pub async fn connect_http_servers(&mut self) -> HashMap<String, Result<()>> {
        let mut results = HashMap::new();
        let ids: Vec<String> = self.http_configs.keys().cloned().collect();

        for id in ids {
            let config = self.http_configs.get(&id).unwrap().clone();
            match connect_http_server(&id, &config).await {
                Ok(toolset) => {
                    tracing::info!("MCP HTTP: connected to '{id}'");
                    self.http_toolsets.push(toolset);
                    results.insert(id, Ok(()));
                }
                Err(e) => {
                    tracing::warn!("MCP HTTP: failed to connect '{id}': {e}");
                    results.insert(id, Err(e));
                }
            }
        }

        results
    }

    /// Build the merged toolset combining stdio + HTTP servers.
    /// Returns None if no servers are configured.
    pub fn toolset(&self) -> Option<Arc<dyn Toolset>> {
        let has_stdio = !self.configs.is_empty();
        let has_http = !self.http_toolsets.is_empty();

        if !has_stdio && !has_http {
            return None;
        }

        let mut toolsets: Vec<Arc<dyn Toolset>> = Vec::new();

        if has_stdio {
            toolsets.push(self.manager.clone());
        }

        for http_ts in &self.http_toolsets {
            toolsets.push(http_ts.clone());
        }

        Some(Arc::new(MergedToolset::new("momo-fetch-mcp", toolsets)))
    }

    /// Start background health monitoring (stdio servers only).
    pub fn start_monitoring(&self) {
        self.manager.start_monitoring();
    }

    /// Stop background health monitoring.
    pub fn stop_monitoring(&self) {
        self.manager.stop_monitoring();
    }

    /// Add a new stdio MCP server, persist config, and optionally auto-start.
    pub async fn add_server(
        &mut self,
        id: String,
        config: McpServerConfig,
        auto_start: bool,
    ) -> Result<()> {
        self.manager
            .add_server(id.clone(), config.clone())
            .await
            .map_err(|e| anyhow!("failed to add MCP server '{id}': {e}"))?;

        self.configs.insert(id.clone(), config);
        self.save_config()?;

        if auto_start {
            self.manager
                .start_server(&id)
                .await
                .map_err(|e| anyhow!("failed to start MCP server '{id}': {e}"))?;
        }

        Ok(())
    }

    /// Remove an MCP server and persist config.
    pub async fn remove_server(&mut self, id: &str) -> Result<()> {
        // Try stdio first
        if self.configs.contains_key(id) {
            self.manager
                .remove_server(id)
                .await
                .map_err(|e| anyhow!("failed to remove MCP server '{id}': {e}"))?;
            self.configs.remove(id);
        }
        // Try HTTP
        if self.http_configs.contains_key(id) {
            self.http_configs.remove(id);
            self.http_toolsets.retain(|ts| ts.name() != id);
        }
        self.save_config()?;
        Ok(())
    }

    /// Start a specific stdio server.
    pub async fn start_server(&self, id: &str) -> Result<()> {
        self.manager
            .start_server(id)
            .await
            .map_err(|e| anyhow!("failed to start MCP server '{id}': {e}"))
    }

    /// Stop a specific stdio server.
    pub async fn stop_server(&self, id: &str) -> Result<()> {
        self.manager
            .stop_server(id)
            .await
            .map_err(|e| anyhow!("failed to stop MCP server '{id}': {e}"))
    }

    /// Get all server statuses (stdio only — HTTP servers are always "running" if connected).
    pub async fn all_statuses(&self) -> HashMap<String, ServerStatus> {
        self.manager.all_statuses().await
    }

    /// Get a specific server status.
    pub async fn server_status(&self, id: &str) -> Result<ServerStatus> {
        self.manager
            .server_status(id)
            .await
            .map_err(|e| anyhow!("failed to get status for '{id}': {e}"))
    }

    /// Count running stdio servers.
    pub async fn running_count(&self) -> usize {
        self.manager.running_server_count().await
    }

    /// Get the underlying stdio manager.
    pub fn manager(&self) -> Arc<McpServerManager> {
        self.manager.clone()
    }

    /// Get the config file path.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Get all configured stdio server IDs and their configs.
    pub fn configs(&self) -> &HashMap<String, McpServerConfig> {
        &self.configs
    }

    /// Get all configured HTTP server configs.
    pub fn http_configs(&self) -> &HashMap<String, HttpMcpServerConfig> {
        &self.http_configs
    }

    /// Get set of connected HTTP server IDs (by matching toolset name against config keys).
    pub fn connected_http_ids(&self) -> HashMap<String, bool> {
        let connected_names: std::collections::HashSet<&str> =
            self.http_toolsets.iter().map(|ts| ts.name()).collect();
        self.http_configs
            .keys()
            .map(|id| (id.clone(), connected_names.contains(id.as_str())))
            .collect()
    }

    /// Check if any servers are configured (stdio or HTTP).
    pub fn has_servers(&self) -> bool {
        !self.configs.is_empty() || !self.http_configs.is_empty()
    }

    /// Check if any HTTP servers are configured.
    pub fn has_http_servers(&self) -> bool {
        !self.http_configs.is_empty()
    }

    /// Gracefully shut down all stdio servers.
    pub async fn shutdown(&self) -> Result<()> {
        self.manager
            .shutdown()
            .await
            .map_err(|e| anyhow!("failed to shutdown MCP servers: {e}"))
    }

    /// Save current stdio configs to `.harness/mcp.json`.
    fn save_config(&self) -> Result<()> {
        if self.config_path.as_os_str().is_empty() {
            return Ok(());
        }

        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json_file = McpJsonFile {
            mcp_servers: self.configs.clone(),
        };
        let json = serde_json::to_string_pretty(&json_file)?;
        std::fs::write(&self.config_path, json)?;
        Ok(())
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_mcp_json_file_parsing() {
        let json = r#"{
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                    "env": {},
                    "disabled": false,
                    "autoApprove": ["read_file"]
                }
            }
        }"#;
        let file: McpJsonFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.mcp_servers.len(), 1);
        assert_eq!(file.mcp_servers["filesystem"].command, "npx");
    }

    #[test]
    fn test_empty_mcp_json() {
        let json = r#"{"mcpServers": {}}"#;
        let file: McpJsonFile = serde_json::from_str(json).unwrap();
        assert!(file.mcp_servers.is_empty());
    }

    #[test]
    fn test_mcp_service_new_empty() {
        let service = McpService::new_empty().unwrap();
        assert!(!service.has_servers());
        assert!(service.configs().is_empty());
    }

    #[tokio::test]
    async fn test_mcp_service_start_all_empty() {
        let service = McpService::new_empty().unwrap();
        let results = service.start_all().await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_service_add_and_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let mut service = McpService::new(tmp.path()).unwrap();

        let config = McpServerConfig {
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            disabled: true,
            auto_approve: vec![],
            restart_policy: None,
        };

        service
            .add_server("test-server".to_string(), config.clone(), false)
            .await
            .unwrap();

        assert!(service.has_servers());
        assert!(service.configs().contains_key("test-server"));

        let config_path = tmp.path().join(".harness").join("mcp.json");
        assert!(config_path.exists());
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("test-server"));
        assert!(content.contains("echo"));

        service.remove_server("test-server").await.unwrap();
        assert!(!service.configs().contains_key("test-server"));

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(!content.contains("test-server"));
    }

    #[test]
    fn test_parse_mcp_config_separates_http_and_stdio() {
        let json = r#"{
            "mcpServers": {
                "zread": {
                    "type": "http",
                    "url": "https://api.z.ai/api/mcp/zread/mcp",
                    "headers": { "Authorization": "Bearer test" }
                },
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@mcp/server-filesystem"],
                    "disabled": false
                }
            }
        }"#;

        let parsed = parse_mcp_config(json).unwrap();
        assert_eq!(parsed.stdio_servers.len(), 1);
        assert_eq!(parsed.stdio_servers["filesystem"].command, "npx");
        assert_eq!(parsed.http_servers.len(), 1);
        assert_eq!(parsed.http_servers["zread"].url, "https://api.z.ai/api/mcp/zread/mcp");
        assert!(parsed.skipped.is_empty());
    }

    #[test]
    fn test_parse_mcp_config_all_http() {
        let json = r#"{
            "mcpServers": {
                "zread": {
                    "type": "http",
                    "url": "https://api.z.ai/api/mcp/zread/mcp"
                }
            }
        }"#;

        let parsed = parse_mcp_config(json).unwrap();
        assert!(parsed.stdio_servers.is_empty());
        assert_eq!(parsed.http_servers.len(), 1);
    }

    #[test]
    fn test_parse_mcp_config_mixed_types() {
        let json = r#"{
            "mcpServers": {
                "stdio-only": {
                    "command": "node",
                    "args": ["server.js"]
                },
                "http-server": {
                    "type": "http",
                    "url": "https://mcp.example.com/api"
                },
                "sse-type": {
                    "type": "sse",
                    "url": "http://localhost:8080/sse"
                },
                "with-command": {
                    "type": "stdio",
                    "command": "python",
                    "args": ["-m", "mcp_server"]
                }
            }
        }"#;

        let parsed = parse_mcp_config(json).unwrap();
        assert_eq!(parsed.stdio_servers.len(), 2);
        assert!(parsed.stdio_servers.contains_key("stdio-only"));
        assert!(parsed.stdio_servers.contains_key("with-command"));
        assert_eq!(parsed.http_servers.len(), 2);
        assert!(parsed.http_servers.contains_key("http-server"));
        assert!(parsed.http_servers.contains_key("sse-type"));
    }

    #[test]
    fn test_parse_mcp_config_disabled_http() {
        let json = r#"{
            "mcpServers": {
                "disabled-http": {
                    "type": "http",
                    "url": "https://example.com",
                    "disabled": true
                }
            }
        }"#;

        let parsed = parse_mcp_config(json).unwrap();
        assert!(parsed.http_servers.is_empty());
        assert_eq!(parsed.skipped.len(), 1);
        assert!(parsed.skipped[0].contains("disabled"));
    }

    #[test]
    fn test_mcp_config_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let config = McpServerConfig {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@anthropic/mcp-filesystem".to_string()],
            env: HashMap::from([("KEY".to_string(), "value".to_string())]),
            disabled: false,
            auto_approve: vec!["read_file".to_string()],
            restart_policy: None,
        };

        let json_file = McpJsonFile {
            mcp_servers: HashMap::from([("filesystem".to_string(), config.clone())]),
        };
        let json = serde_json::to_string_pretty(&json_file).unwrap();
        let config_path = tmp.path().join("mcp.json");
        std::fs::write(&config_path, &json).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        let parsed: McpJsonFile = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.mcp_servers["filesystem"].command, "npx");
        assert_eq!(parsed.mcp_servers["filesystem"].args.len(), 2);
    }

    #[test]
    fn test_http_mcp_server_config_parsing() {
        let json = r#"{
            "type": "http",
            "url": "https://api.z.ai/mcp",
            "headers": { "Authorization": "Bearer token123" },
            "disabled": false
        }"#;

        let config: HttpMcpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.server_type, "http");
        assert_eq!(config.url, "https://api.z.ai/mcp");
        assert_eq!(config.headers.len(), 1);
        assert!(!config.disabled);
    }
}
