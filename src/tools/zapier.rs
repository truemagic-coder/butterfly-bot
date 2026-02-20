use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::{Tool, ToolSecret};
use crate::tools::mcp::McpTool;
use crate::vault;

#[derive(Clone, Debug)]
struct ZapierConfig {
    url: String,
    headers: HashMap<String, String>,
    token: Option<String>,
}

impl Default for ZapierConfig {
    fn default() -> Self {
        Self {
            url: "https://mcp.zapier.com/api/v1/connect".to_string(),
            headers: HashMap::new(),
            token: None,
        }
    }
}

pub struct ZapierTool {
    config: RwLock<ZapierConfig>,
}

impl Default for ZapierTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ZapierTool {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(ZapierConfig::default()),
        }
    }

    fn get_tool_config(config: &Value) -> Option<&Value> {
        config.get("tools").and_then(|tools| tools.get("zapier"))
    }

    fn parse_headers(value: &Value) -> HashMap<String, String> {
        value
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default()
    }

    fn has_token_in_url(url: &str) -> bool {
        url.contains("token=")
    }

    fn is_placeholder_token(token: &str) -> bool {
        let trimmed = token.trim();
        trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("my_token")
            || trimmed.eq_ignore_ascii_case("your_zapier_token")
            || trimmed.eq_ignore_ascii_case("token")
    }

    fn has_authorization_header(headers: &HashMap<String, String>) -> bool {
        headers
            .keys()
            .any(|key| key.eq_ignore_ascii_case("authorization"))
    }

    fn set_bearer_header(headers: &mut HashMap<String, String>, token: &str) {
        if !Self::has_authorization_header(headers) {
            headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        }
    }

    fn token_from_url(url: &str) -> Option<String> {
        let query = url.split_once('?')?.1;
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;
            if key == "token" && !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
        None
    }

    fn has_valid_token_in_url(url: &str) -> bool {
        Self::token_from_url(url)
            .map(|token| !Self::is_placeholder_token(&token))
            .unwrap_or(false)
    }

    fn url_with_token(url: &str, token: &str) -> String {
        if Self::has_token_in_url(url) {
            return url.to_string();
        }
        let separator = if url.contains('?') { '&' } else { '?' };
        format!("{url}{separator}token={token}")
    }

    fn build_mcp_config(&self, config: &ZapierConfig) -> Value {
        json!({
            "tools": {
                "mcp": {
                    "servers": [
                        {
                            "name": "zapier",
                            "url": config.url.clone(),
                            "headers": config.headers.clone()
                        }
                    ]
                }
            }
        })
    }
}

#[async_trait]
impl Tool for ZapierTool {
    fn name(&self) -> &str {
        "zapier"
    }

    fn description(&self) -> &str {
        "Access Zapier MCP tools through a dedicated MCP endpoint wrapper."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_tools", "call_tool"]
                },
                "tool": { "type": "string", "description": "Zapier MCP tool name" },
                "arguments": { "type": "object", "description": "Arguments for the Zapier MCP tool" }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn required_secrets_for_config(&self, config: &Value) -> Vec<ToolSecret> {
        let Some(tool_cfg) = Self::get_tool_config(config) else {
            return Vec::new();
        };

        let url = tool_cfg
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://mcp.zapier.com/api/v1/connect");
        let has_inline_token = tool_cfg.get("token").and_then(|v| v.as_str()).is_some();
        let has_auth_header = tool_cfg
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|headers| {
                headers
                    .keys()
                    .any(|key| key.eq_ignore_ascii_case("authorization"))
            })
            .unwrap_or(false);
        let has_valid_inline_token = tool_cfg
            .get("token")
            .and_then(|v| v.as_str())
            .map(|token| !Self::is_placeholder_token(token))
            .unwrap_or(false);

        if Self::has_valid_token_in_url(url)
            || has_valid_inline_token
            || (has_auth_header && !has_inline_token)
        {
            Vec::new()
        } else {
            vec![ToolSecret::new("zapier_token", "Zapier MCP token")]
        }
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let mut next = ZapierConfig::default();

        if let Some(tool_cfg) = Self::get_tool_config(config) {
            if let Some(url) = tool_cfg.get("url").and_then(|v| v.as_str()) {
                if !url.trim().is_empty() {
                    next.url = url.to_string();
                }
            }
            if let Some(token) = tool_cfg.get("token").and_then(|v| v.as_str()) {
                if !Self::is_placeholder_token(token) {
                    next.token = Some(token.to_string());
                }
            }
            if let Some(headers) = tool_cfg.get("headers") {
                next.headers = Self::parse_headers(headers);
            }
        }

        if let Some(token) = next.token.clone() {
            next.url = Self::url_with_token(&next.url, &token);
            Self::set_bearer_header(&mut next.headers, &token);
        } else if let Some(token) = Self::token_from_url(&next.url) {
            if Self::is_placeholder_token(&token) {
                next.url = "https://mcp.zapier.com/api/v1/connect".to_string();
            } else {
                Self::set_bearer_header(&mut next.headers, &token);
            }
        }

        if !Self::has_valid_token_in_url(&next.url)
            && next
                .headers
                .get("Authorization")
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        {
            next.url = "https://mcp.zapier.com/api/v1/connect".to_string();
        }

        let mut guard = self
            .config
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("Zapier tool lock busy".to_string()))?;
        *guard = next;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut config = self.config.read().await.clone();

        if !Self::has_valid_token_in_url(&config.url)
            && !Self::has_authorization_header(&config.headers)
        {
            if let Some(secret) = vault::get_secret("zapier_token")? {
                if !Self::is_placeholder_token(&secret) {
                    config.url = Self::url_with_token(&config.url, &secret);
                    Self::set_bearer_header(&mut config.headers, &secret);
                }
            }
        }

        if !Self::has_valid_token_in_url(&config.url)
            && !Self::has_authorization_header(&config.headers)
        {
            return Err(ButterflyBotError::Runtime(
                "Missing Zapier token (set tools.zapier.token, tools.zapier.headers.Authorization, tools.zapier.url with token=..., or vault zapier_token)"
                    .to_string(),
            ));
        }

        let mcp_config = self.build_mcp_config(&config);
        let mcp_tool = McpTool::new();
        mcp_tool.configure(&mcp_config)?;

        match action.as_str() {
            "list_tools" => {
                let result = mcp_tool
                    .execute(json!({"action": "list_tools", "server": "zapier"}))
                    .await?;
                Ok(result)
            }
            "call_tool" => {
                let tool_name = params
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing tool name".to_string()))?;
                let args = params.get("arguments").cloned();
                let result = mcp_tool
                    .execute(json!({
                        "action": "call_tool",
                        "server": "zapier",
                        "tool": tool_name,
                        "arguments": args
                    }))
                    .await?;
                Ok(result)
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}
