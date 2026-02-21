use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::info;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;

#[derive(Clone, Debug, Default)]
struct HttpCallServerConfig {
    name: String,
    url: String,
    headers: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
struct HttpCallConfig {
    servers: Vec<HttpCallServerConfig>,
    timeout_seconds: Option<u64>,
}

pub struct HttpCallTool {
    config: RwLock<HttpCallConfig>,
}

impl Default for HttpCallTool {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpCallTool {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(HttpCallConfig::default()),
        }
    }

    fn build_headers(
        default_headers: &HashMap<String, String>,
        headers: Option<&Value>,
    ) -> Result<HeaderMap> {
        let mut out = HeaderMap::new();
        for (key, value) in default_headers {
            let header_name = key
                .parse::<reqwest::header::HeaderName>()
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            let header_value = value
                .parse::<reqwest::header::HeaderValue>()
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            out.insert(header_name, header_value);
        }
        if let Some(headers) = headers.and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(value) = value.as_str() {
                    let header_name = key
                        .parse::<reqwest::header::HeaderName>()
                        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                    let header_value = value
                        .parse::<reqwest::header::HeaderValue>()
                        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                    out.insert(header_name, header_value);
                }
            }
        }
        Ok(out)
    }

    fn build_url(
        server: Option<&HttpCallServerConfig>,
        url: Option<&str>,
        endpoint: Option<&str>,
    ) -> Result<String> {
        if let Some(url) = url {
            if !url.trim().is_empty() {
                return Ok(url.trim().to_string());
            }
        }
        let endpoint = endpoint.unwrap_or("").trim();
        if endpoint.is_empty() {
            return Err(ButterflyBotError::Runtime(
                "Missing url or endpoint".to_string(),
            ));
        }
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            return Ok(endpoint.to_string());
        }
        let base = server
            .map(|s| s.url.as_str())
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                ButterflyBotError::Runtime(
                    "Missing server for endpoint; configure tools.http_call.servers".to_string(),
                )
            })?;
        let endpoint = endpoint.trim_start_matches('/');
        Ok(format!("{base}/{endpoint}"))
    }

    fn apply_query(req: reqwest::RequestBuilder, query: Option<&Value>) -> reqwest::RequestBuilder {
        if let Some(map) = query.and_then(|v| v.as_object()) {
            let pairs: Vec<(String, String)> = map
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or(&v.to_string()).to_string()))
                .collect();
            return req.query(&pairs);
        }
        req
    }

    fn redact_headers(headers: &HeaderMap) -> HashMap<String, String> {
        headers
            .iter()
            .map(|(k, v)| {
                let key = k.to_string();
                let lower = key.to_ascii_lowercase();
                let value = if lower.contains("authorization")
                    || lower.contains("api-key")
                    || lower.contains("apikey")
                    || lower.contains("token")
                    || lower.contains("secret")
                {
                    "[REDACTED]".to_string()
                } else {
                    v.to_str().unwrap_or("").to_string()
                };
                (key, value)
            })
            .collect()
    }

    fn parse_headers(value: Option<&Value>) -> HashMap<String, String> {
        value
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| {
                        v.as_str()
                            .map(|value| (k.trim().to_string(), value.trim().to_string()))
                    })
                    .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default()
    }

    fn parse_servers(cfg: &Value) -> Result<Vec<HttpCallServerConfig>> {
        let mut parsed = Vec::new();
        if let Some(servers) = cfg.get("servers").and_then(|v| v.as_array()) {
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
                        "HTTP call server entry requires name and url".to_string(),
                    ));
                }
                parsed.push(HttpCallServerConfig {
                    name,
                    url,
                    headers: Self::parse_headers(server.get("headers")),
                });
            }
            return Ok(parsed);
        }

        let mut shared_headers = Self::parse_headers(cfg.get("custom_headers"));
        let default_headers = Self::parse_headers(cfg.get("default_headers"));
        for (key, value) in default_headers {
            shared_headers.entry(key).or_insert(value);
        }

        if let Some(base_urls) = cfg.get("base_urls").and_then(|v| v.as_array()) {
            for (index, value) in base_urls.iter().enumerate() {
                let url = value.as_str().unwrap_or_default().trim().to_string();
                if url.is_empty() {
                    continue;
                }
                parsed.push(HttpCallServerConfig {
                    name: format!("server_{}", index + 1),
                    url,
                    headers: shared_headers.clone(),
                });
            }
            if !parsed.is_empty() {
                return Ok(parsed);
            }
        }

        if let Some(base_url) = cfg.get("base_url").and_then(|v| v.as_str()) {
            let url = base_url.trim().to_string();
            if !url.is_empty() {
                parsed.push(HttpCallServerConfig {
                    name: "default".to_string(),
                    url,
                    headers: shared_headers,
                });
            }
        }

        Ok(parsed)
    }

    fn find_server<'a>(
        servers: &'a [HttpCallServerConfig],
        name: Option<&str>,
    ) -> Result<Option<&'a HttpCallServerConfig>> {
        if servers.is_empty() {
            if let Some(name) = name {
                if !name.trim().is_empty() {
                    return Err(ButterflyBotError::Runtime(
                        "No HTTP call servers configured".to_string(),
                    ));
                }
            }
            return Ok(None);
        }

        if let Some(name) = name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                if let Some(server) = servers.iter().find(|s| s.name == trimmed) {
                    return Ok(Some(server));
                }
                return Err(ButterflyBotError::Runtime(format!(
                    "Unknown HTTP call server '{trimmed}'"
                )));
            }
        }

        if servers.len() == 1 {
            Ok(Some(&servers[0]))
        } else {
            Err(ButterflyBotError::Runtime(
                "Multiple HTTP call servers configured; specify server name".to_string(),
            ))
        }
    }
}

#[async_trait]
impl Tool for HttpCallTool {
    fn name(&self) -> &str {
        "http_call"
    }

    fn description(&self) -> &str {
        "Perform arbitrary HTTP requests with custom headers and optional JSON/body payloads."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "method": { "type": "string" },
                "server": { "type": "string", "description": "HTTP server name from config" },
                "url": { "type": "string" },
                "endpoint": { "type": "string" },
                "headers": { "type": "object" },
                "query": { "type": "object" },
                "body": { "type": "string" },
                "json": { "type": "object" },
                "timeout_seconds": { "type": "integer" }
            },
            "required": ["method"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let tool_cfg = config.get("tools").and_then(|v| v.get("http_call"));
        let mut next = HttpCallConfig::default();
        if let Some(cfg) = tool_cfg {
            next.servers = Self::parse_servers(cfg)?;
            if let Some(timeout) = cfg.get("timeout_seconds").and_then(|v| v.as_u64()) {
                next.timeout_seconds = Some(timeout);
            }
        }

        let mut guard = self
            .config
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("HTTP call tool lock busy".to_string()))?;
        *guard = next;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let method = params
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_uppercase();
        if method.is_empty() {
            return Err(ButterflyBotError::Runtime("Missing method".to_string()));
        }

        let server_name = params.get("server").and_then(|v| v.as_str());
        let url = params.get("url").and_then(|v| v.as_str());
        let endpoint = params.get("endpoint").and_then(|v| v.as_str());
        let headers = params.get("headers");
        let query = params.get("query");
        let body = params
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let json_body = params.get("json").cloned();
        let timeout_override = params.get("timeout_seconds").and_then(|v| v.as_u64());

        let cfg = self.config.read().await.clone();
        let selected_server = Self::find_server(&cfg.servers, server_name)?;
        let url = Self::build_url(selected_server, url, endpoint)?;
        let default_headers = selected_server
            .map(|server| server.headers.clone())
            .unwrap_or_default();
        let mut headers = Self::build_headers(&default_headers, headers)?;
        let mut body = body;
        let mut inferred_json: Option<Value> = None;
        if json_body.is_none() {
            if let Some(body_str) = body.as_deref() {
                if !headers.contains_key(CONTENT_TYPE) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(body_str) {
                        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                        inferred_json = Some(parsed);
                        body = None;
                    }
                }
            }
        }

        let client = reqwest::Client::new();
        let mut req = client.request(
            method
                .parse()
                .map_err(|_| ButterflyBotError::Runtime("Invalid method".to_string()))?,
            &url,
        );

        let redacted_headers = Self::redact_headers(&headers);
        if !headers.is_empty() {
            req = req.headers(headers);
        }
        req = Self::apply_query(req, query);

        if let Some(json_body) = json_body.and_then(|v| v.as_object().cloned()) {
            req = req.json(&json_body);
        } else if let Some(inferred_json) = inferred_json {
            req = req.json(&inferred_json);
        } else if let Some(body) = body {
            req = req.body(body);
        }

        let timeout = timeout_override.or(cfg.timeout_seconds).unwrap_or(60);
        req = req.timeout(Duration::from_secs(timeout));

        info!(
            method = %method,
            url = %url,
            headers = ?redacted_headers,
            "HTTP call request"
        );

        let response = req
            .send()
            .await
            .map_err(|e| ButterflyBotError::Http(e.to_string()))?;

        let status = response.status().as_u16();
        info!(
            method = %method,
            url = %url,
            status = %status,
            "HTTP call response"
        );
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect::<HashMap<_, _>>();

        let text = response
            .text()
            .await
            .map_err(|e| ButterflyBotError::Http(e.to_string()))?;
        let json_value = serde_json::from_str::<Value>(&text).ok();

        Ok(json!({
            "status": "ok",
            "server": selected_server.map(|server| server.name.clone()),
            "http_status": status,
            "headers": headers,
            "text": text,
            "json": json_value
        }))
    }
}
