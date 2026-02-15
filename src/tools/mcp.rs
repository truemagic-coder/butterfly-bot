use std::collections::HashMap;

use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, PaginatedRequestParams};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use reqwest_mcp::header::{HeaderMap, HeaderName, HeaderValue};
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
            parsed.push(McpServerConfig {
                name,
                url,
                headers,
            });
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

    fn build_http_client(server: &McpServerConfig) -> Result<reqwest_mcp::Client> {
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

        reqwest_mcp::Client::builder()
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
            Box<dyn std::future::Future<Output = std::result::Result<T, rmcp::ServiceError>> + Send + 'a>,
        >,
    {
        let client = Self::build_http_client(server)?;
        let mut transport_config = StreamableHttpClientTransportConfig::with_uri(server.url.clone());
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
        if let Err(err) = shutdown {
            return Err(err);
        }

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
            .await
            ?;
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
