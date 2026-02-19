use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fs;

use crate::error::{ButterflyBotError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenAiConfig {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    pub enabled: Option<bool>,
    pub sqlite_path: Option<String>,
    pub summary_model: Option<String>,
    pub embedding_model: Option<String>,
    pub rerank_model: Option<String>,
    pub openai: Option<OpenAiConfig>,
    pub context_embed_enabled: Option<bool>,
    pub summary_threshold: Option<usize>,
    pub retention_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarkdownSource {
    Url { url: String },
    Database { markdown: String },
}

impl MarkdownSource {
    pub fn default_heartbeat() -> Self {
        Self::Database {
            markdown: "# Heartbeat\n\nStay proactive, grounded, and transparent. Prefer clear next steps and avoid over-claiming.".to_string(),
        }
    }

    pub fn default_prompt() -> Self {
        Self::Database {
            markdown: "# Prompt\n\nAnswer directly, include concrete actions, and keep responses practical.".to_string(),
        }
    }

    pub fn as_url(&self) -> Option<&str> {
        match self {
            Self::Url { url } => Some(url.as_str()),
            Self::Database { .. } => None,
        }
    }

    pub fn as_database_markdown(&self) -> Option<&str> {
        match self {
            Self::Url { .. } => None,
            Self::Database { markdown } => Some(markdown.as_str()),
        }
    }

    fn from_legacy_string(value: String) -> Self {
        let trimmed = value.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Self::Url {
                url: trimmed.to_string(),
            };
        }
        let markdown = fs::read_to_string(trimmed).unwrap_or_default();
        Self::Database { markdown }
    }

    fn from_json_value(value: Value) -> std::result::Result<Self, String> {
        match value {
            Value::String(raw) => Ok(Self::from_legacy_string(raw)),
            Value::Object(map) => {
                if let Some(kind) = map.get("type").and_then(|v| v.as_str()) {
                    match kind {
                        "url" => {
                            let url = map
                                .get("url")
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| "url source requires `url`".to_string())?;
                            Ok(Self::Url {
                                url: url.to_string(),
                            })
                        }
                        "database" => {
                            let markdown = map
                                .get("markdown")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            Ok(Self::Database {
                                markdown: markdown.to_string(),
                            })
                        }
                        other => Err(format!("unsupported markdown source type: {other}")),
                    }
                } else if let Some(url) = map.get("url").and_then(|v| v.as_str()) {
                    Ok(Self::Url {
                        url: url.to_string(),
                    })
                } else if let Some(markdown) = map.get("markdown").and_then(|v| v.as_str()) {
                    Ok(Self::Database {
                        markdown: markdown.to_string(),
                    })
                } else {
                    Err(
                        "markdown source object must include `type` or (`url`/`markdown`)"
                            .to_string(),
                    )
                }
            }
            Value::Null => Err("markdown source cannot be null".to_string()),
            other => Err(format!("invalid markdown source: {other}")),
        }
    }
}

impl<'de> Deserialize<'de> for MarkdownSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_json_value(value).map_err(de::Error::custom)
    }
}

fn default_heartbeat_source() -> MarkdownSource {
    MarkdownSource::default_heartbeat()
}

fn default_prompt_source() -> MarkdownSource {
    MarkdownSource::default_prompt()
}

fn deserialize_heartbeat_source<'de, D>(
    deserializer: D,
) -> std::result::Result<MarkdownSource, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(default_heartbeat_source()),
        Some(value) => MarkdownSource::from_json_value(value).map_err(de::Error::custom),
    }
}

fn deserialize_prompt_source<'de, D>(
    deserializer: D,
) -> std::result::Result<MarkdownSource, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(default_prompt_source()),
        Some(value) => MarkdownSource::from_json_value(value).map_err(de::Error::custom),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub openai: Option<OpenAiConfig>,
    #[serde(
        default = "default_heartbeat_source",
        alias = "heartbeat_file",
        deserialize_with = "deserialize_heartbeat_source"
    )]
    pub heartbeat_source: MarkdownSource,
    #[serde(
        default = "default_prompt_source",
        alias = "prompt_file",
        deserialize_with = "deserialize_prompt_source"
    )]
    pub prompt_source: MarkdownSource,
    pub memory: Option<MemoryConfig>,
    pub tools: Option<Value>,
    pub brains: Option<Value>,
}
impl Config {
    fn apply_security_defaults(mut self) -> Self {
        let tools = self.tools.get_or_insert_with(|| Value::Object(Map::new()));
        if let Some(tools_obj) = tools.as_object_mut() {
            let settings = tools_obj
                .entry("settings")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(settings_obj) = settings.as_object_mut() {
                let permissions = settings_obj
                    .entry("permissions")
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(perms_obj) = permissions.as_object_mut() {
                    perms_obj
                        .entry("default_deny")
                        .or_insert_with(|| Value::Bool(true));
                    perms_obj.entry("network_allow").or_insert_with(|| {
                        Value::Array(vec![
                            Value::String("localhost".to_string()),
                            Value::String("127.0.0.1".to_string()),
                            Value::String("api.openai.com".to_string()),
                            Value::String("api.x.ai".to_string()),
                            Value::String("api.perplexity.ai".to_string()),
                            Value::String("api.githubcopilot.com".to_string()),
                            Value::String("mcp.zapier.com".to_string()),
                        ])
                    });
                }

                let solana = settings_obj
                    .entry("solana")
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(solana_obj) = solana.as_object_mut() {
                    solana_obj.entry("rpc").or_insert_with(|| {
                        serde_json::json!({
                            "provider": "quicknode",
                            "endpoint": "",
                            "commitment": "confirmed",
                            "bootstrap_wallets": [
                                {
                                    "user_id": "user",
                                    "actor": "agent"
                                }
                            ],
                            "compute_budget": {
                                "unit_limit": 300000,
                                "unit_price_microlamports": 2500
                            },
                            "simulation": {
                                "enabled": true,
                                "commitment": "processed",
                                "replace_recent_blockhash": true,
                                "sig_verify": false,
                                "min_context_slot": null
                            },
                            "send": {
                                "skip_preflight": false,
                                "preflight_commitment": "confirmed",
                                "max_retries": 5
                            }
                        })
                    });
                }
            }
        }
        self
    }

    pub fn convention_defaults(db_path: &str) -> Self {
        let model = "ministral-3:14b".to_string();
        Self {
            openai: Some(OpenAiConfig {
                api_key: None,
                model: Some(model.clone()),
                base_url: Some("http://localhost:11434/v1".to_string()),
            }),
            heartbeat_source: default_heartbeat_source(),
            prompt_source: default_prompt_source(),
            memory: Some(MemoryConfig {
                enabled: Some(true),
                sqlite_path: Some(db_path.to_string()),
                summary_model: Some(model),
                embedding_model: Some("embeddinggemma:latest".to_string()),
                rerank_model: Some("qllama/bge-reranker-v2-m3".to_string()),
                openai: None,
                context_embed_enabled: Some(false),
                summary_threshold: None,
                retention_days: None,
            }),
            tools: Some(Value::Object(Map::new())),
            brains: None,
        }
        .apply_security_defaults()
    }

    pub fn from_store(db_path: &str) -> Result<Self> {
        match crate::config_store::load_config(db_path) {
            Ok(config) => Ok(config.apply_security_defaults()),
            Err(store_err) => {
                if let Some(secret) = crate::vault::get_secret("app_config_json")? {
                    if !secret.trim().is_empty() {
                        let value: Value = serde_json::from_str(&secret)
                            .map_err(|e| ButterflyBotError::Config(e.to_string()))?;
                        let config: Config = serde_json::from_value(value)
                            .map_err(|e| ButterflyBotError::Config(e.to_string()))?;
                        return Ok(config.apply_security_defaults());
                    }
                }

                Err(store_err)
            }
        }
    }

    pub fn resolve_vault(mut self) -> Result<Self> {
        if let Some(openai) = &mut self.openai {
            if openai.api_key.is_none() {
                if let Some(secret) = crate::vault::get_secret("openai_api_key")? {
                    openai.api_key = Some(secret);
                }
            }
        }
        if let Some(memory) = &mut self.memory {
            if let Some(openai) = &mut memory.openai {
                if openai.api_key.is_none() {
                    if let Some(secret) = crate::vault::get_secret("memory_openai_api_key")? {
                        openai.api_key = Some(secret);
                    }
                }
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convention_defaults_include_solana_rpc_settings() {
        let config = Config::convention_defaults(":memory:");
        let tools = config.tools.expect("tools should be initialized");

        let rpc = tools
            .get("settings")
            .and_then(|settings| settings.get("solana"))
            .and_then(|solana| solana.get("rpc"))
            .expect("tools.settings.solana.rpc should exist");

        assert_eq!(
            rpc.get("provider").and_then(|v| v.as_str()),
            Some("quicknode")
        );
        assert_eq!(
            rpc.get("commitment").and_then(|v| v.as_str()),
            Some("confirmed")
        );

        let simulation = rpc
            .get("simulation")
            .and_then(|v| v.as_object())
            .expect("simulation object should exist");
        assert_eq!(simulation.get("enabled").and_then(|v| v.as_bool()), Some(true));

        let send = rpc
            .get("send")
            .and_then(|v| v.as_object())
            .expect("send object should exist");
        assert_eq!(
            send.get("skip_preflight").and_then(|v| v.as_bool()),
            Some(false)
        );
    }
}
