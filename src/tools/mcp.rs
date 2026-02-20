use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;

#[derive(Clone, Debug)]
struct McpServerConfig {
    name: String,
    url: String,
    headers: HashMap<String, String>,
}

pub struct McpTool {
    servers: RwLock<Vec<McpServerConfig>>,
}

impl Default for McpTool {
    fn default() -> Self {
        Self::new()
    }
}

impl McpTool {
    pub fn new() -> Self {
        Self {
            servers: RwLock::new(Vec::new()),
        }
    }

    fn parse_servers(config: &Value) -> Result<Vec<McpServerConfig>> {
        let Some(servers) = config
            .get("tools")
            .and_then(|tools| tools.get("mcp"))
            .and_then(|mcp| mcp.get("servers"))
            .and_then(|servers| servers.as_array())
        else {
            return Ok(Vec::new());
        };

        let mut parsed = Vec::new();
        for server in servers {
            let name = server
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            let url = server
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if name.is_empty() || url.is_empty() {
                return Err(ButterflyBotError::Config(
                    "MCP server entry requires name and url".to_string(),
                ));
            }
            let headers = server
                .get("headers")
                .and_then(|v| v.as_object())
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect::<HashMap<_, _>>()
                })
                .unwrap_or_default();
            parsed.push(McpServerConfig { name, url, headers });
        }
        Ok(parsed)
    }

    async fn find_server(&self, name: Option<&str>) -> Result<McpServerConfig> {
        let servers = self.servers.read().await;
        if servers.is_empty() {
            return Err(ButterflyBotError::Runtime(
                "No MCP servers configured".to_string(),
            ));
        }
        if let Some(name) = name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                if let Some(server) = servers.iter().find(|s| s.name == trimmed) {
                    return Ok(server.clone());
                }
                return Err(ButterflyBotError::Runtime(format!(
                    "Unknown MCP server '{trimmed}'"
                )));
            }
        }
        if servers.len() == 1 {
            Ok(servers[0].clone())
        } else {
            Err(ButterflyBotError::Runtime(
                "Multiple MCP servers configured; specify server name".to_string(),
            ))
        }
    }

    fn build_http_client(server: &McpServerConfig) -> Result<reqwest::Client> {
        let mut headers = HeaderMap::new();
        for (key, value) in &server.headers {
            let name = HeaderName::from_bytes(key.as_bytes()).map_err(|err| {
                ButterflyBotError::Config(format!("Invalid MCP header name '{}': {err}", key))
            })?;
            let value = HeaderValue::from_str(value).map_err(|err| {
                ButterflyBotError::Config(format!("Invalid MCP header value for '{}': {err}", key))
            })?;
            headers.insert(name, value);
        }

        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|err| ButterflyBotError::Runtime(err.to_string()))
    }

    async fn with_client<T, F>(&self, server: &McpServerConfig, operation: F) -> Result<T>
    where
        F: for<'a> FnOnce(
            &'a rmcp::service::Peer<rmcp::RoleClient>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = std::result::Result<T, rmcp::ServiceError>>
                    + Send
                    + 'a,
            >,
        >,
    {
        let client = Self::build_http_client(server)?;
        let mut transport_config =
            StreamableHttpClientTransportConfig::with_uri(server.url.clone());
        transport_config.allow_stateless = true;
        let transport = StreamableHttpClientTransport::with_client(client, transport_config);
        let runtime = ()
            .serve(transport)
            .await
            .map_err(|err| ButterflyBotError::Runtime(err.to_string()))?;

        let result = operation(runtime.peer())
            .await
            .map_err(|err| ButterflyBotError::Runtime(err.to_string()));

        let shutdown = runtime
            .cancel()
            .await
            .map_err(|err| ButterflyBotError::Runtime(err.to_string()));
        shutdown?;

        result
    }

    async fn list_tools(&self, server: &McpServerConfig) -> Result<Value> {
        let list = self
            .with_client(server, |peer| {
                Box::pin(async move {
                    peer.list_tools(Some(PaginatedRequestParams {
                        meta: None,
                        cursor: None,
                    }))
                    .await
                })
            })
            .await?;
        serde_json::to_value(&list).map_err(|e| ButterflyBotError::Serialization(e.to_string()))
    }

    async fn call_tool(
        &self,
        server: &McpServerConfig,
        tool_name: &str,
        args: Option<Value>,
    ) -> Result<Value> {
        let args_map = args.and_then(|value| value.as_object().cloned());
        let tool_name = tool_name.to_string();
        let result = self
            .with_client(server, |peer| {
                Box::pin(async move {
                    peer.call_tool(CallToolRequestParams {
                        name: tool_name.into(),
                        arguments: args_map,
                        meta: None,
                        task: None,
                    })
                    .await
                })
            })
            .await?;
        serde_json::to_value(&result).map_err(|e| ButterflyBotError::Serialization(e.to_string()))
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Call tools on configured MCP servers (streamable HTTP)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_tools", "call_tool"]
                },
                "server": { "type": "string", "description": "MCP server name from config" },
                "tool": { "type": "string", "description": "Tool name to invoke on MCP server" },
                "arguments": { "type": "object", "description": "Arguments for the MCP tool" }
            },
            "required": ["action"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let servers = Self::parse_servers(config)?;
        let mut guard = self
            .servers
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("MCP tool lock busy".to_string()))?;
        *guard = servers;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let server_name = params.get("server").and_then(|v| v.as_str());
        let server = self.find_server(server_name).await?;

        match action.as_str() {
            "list_tools" => {
                let list = self.list_tools(&server).await?;
                Ok(json!({"status": "ok", "server": server.name, "tools": list}))
            }
            "call_tool" => {
                let tool_name = params
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing tool name".to_string()))?;
                let args = params.get("arguments").cloned();
                let result = self.call_tool(&server, tool_name, args).await?;
                Ok(json!({"status": "ok", "server": server.name, "result": result}))
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_message(err: ButterflyBotError) -> String {
        match err {
            ButterflyBotError::Runtime(msg) => msg,
            other => panic!("expected runtime error, got {other}"),
        }
    }

    #[test]
    fn parse_servers_handles_empty_and_invalid_entries() {
        let empty = McpTool::parse_servers(&json!({})).expect("empty config should parse");
        assert!(empty.is_empty());

        let err = McpTool::parse_servers(&json!({
            "tools": {"mcp": {"servers": [{"name": "only-name"}]}}
        }))
        .expect_err("server entry without url should fail");
        match err {
            ButterflyBotError::Config(msg) => assert!(msg.contains("requires name and url")),
            other => panic!("expected config error, got {other}"),
        }
    }

    #[test]
    fn parse_servers_keeps_string_headers_only() {
        let servers = McpTool::parse_servers(&json!({
            "tools": {
                "mcp": {
                    "servers": [
                        {
                            "name": "demo",
                            "url": "http://localhost:3001",
                            "headers": {
                                "x-api-key": "abc",
                                "x-bool": true,
                                "x-num": 42
                            }
                        }
                    ]
                }
            }
        }))
        .expect("valid server should parse");

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "demo");
        assert_eq!(servers[0].url, "http://localhost:3001");
        assert_eq!(servers[0].headers.len(), 1);
        assert_eq!(
            servers[0].headers.get("x-api-key"),
            Some(&"abc".to_string())
        );
    }

    #[tokio::test]
    async fn find_server_routes_single_and_named_servers() {
        let tool = McpTool::new();
        tool.configure(&json!({
            "tools": {
                "mcp": {
                    "servers": [
                        {"name": "primary", "url": "http://localhost:3001"}
                    ]
                }
            }
        }))
        .expect("configure single server");

        let auto = tool
            .find_server(None)
            .await
            .expect("single server auto selection");
        assert_eq!(auto.name, "primary");

        let named = tool
            .find_server(Some("primary"))
            .await
            .expect("named server lookup");
        assert_eq!(named.url, "http://localhost:3001");

        let err = tool
            .find_server(Some("missing"))
            .await
            .expect_err("unknown server should fail");
        assert!(runtime_message(err).contains("Unknown MCP server"));
    }

    #[tokio::test]
    async fn find_server_requires_name_when_multiple() {
        let tool = McpTool::new();
        tool.configure(&json!({
            "tools": {
                "mcp": {
                    "servers": [
                        {"name": "a", "url": "http://localhost:3001"},
                        {"name": "b", "url": "http://localhost:3002"}
                    ]
                }
            }
        }))
        .expect("configure multiple servers");

        let err = tool
            .find_server(Some("   "))
            .await
            .expect_err("blank server should fail with multiple configured");
        assert!(runtime_message(err).contains("Multiple MCP servers configured"));
    }

    #[test]
    fn build_http_client_validates_headers() {
        let valid_server = McpServerConfig {
            name: "ok".to_string(),
            url: "http://localhost:3001".to_string(),
            headers: HashMap::from([("x-token".to_string(), "abc".to_string())]),
        };
        McpTool::build_http_client(&valid_server).expect("valid headers should build client");

        let invalid_server = McpServerConfig {
            name: "bad".to_string(),
            url: "http://localhost:3001".to_string(),
            headers: HashMap::from([("bad header".to_string(), "abc".to_string())]),
        };
        let err = McpTool::build_http_client(&invalid_server).expect_err("invalid header");
        match err {
            ButterflyBotError::Config(msg) => assert!(msg.contains("Invalid MCP header name")),
            other => panic!("expected config error, got {other}"),
        }
    }

    #[test]
    fn parameters_schema_includes_actions() {
        let schema = McpTool::new().parameters();
        assert_eq!(schema["required"], json!(["action"]));
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert!(actions.iter().any(|v| v == "list_tools"));
        assert!(actions.iter().any(|v| v == "call_tool"));
    }
}
