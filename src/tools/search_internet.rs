use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::error::Result;
use crate::interfaces::plugins::{Tool, ToolSecret};
use crate::vault;

#[derive(Debug, Clone)]
struct SearchInternetState {
    api_key: Option<String>,
    provider: String,
    model: String,
    citations: bool,
    grok_web_search: bool,
    grok_x_search: bool,
    grok_timeout: u64,
    network_allow: Vec<String>,
    default_deny: bool,
}

impl Default for SearchInternetState {
    fn default() -> Self {
        Self {
            api_key: None,
            provider: "openai".to_string(),
            model: "".to_string(),
            citations: true,
            grok_web_search: true,
            grok_x_search: true,
            grok_timeout: 90,
            network_allow: Vec::new(),
            default_deny: false,
        }
    }
}

pub struct SearchInternetTool {
    state: Mutex<SearchInternetState>,
}

impl SearchInternetTool {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SearchInternetState::default()),
        }
    }

    fn snapshot(&self) -> SearchInternetState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    fn set_defaults(state: &mut SearchInternetState, model_set_from_config: bool) {
        if model_set_from_config {
            return;
        }
        match state.provider.as_str() {
            "perplexity" => state.model = "sonar".to_string(),
            "openai" => state.model = "gpt-4o-mini-search-preview".to_string(),
            "grok" => state.model = "grok-4-1-fast-non-reasoning".to_string(),
            _ => state.model = "".to_string(),
        }
    }

    fn get_tool_config(config: &Value) -> Option<&Value> {
        config
            .get("tools")
            .and_then(|tools| tools.get("search_internet"))
    }

    fn get_settings_config(config: &Value) -> Option<&Value> {
        config.get("tools").and_then(|tools| tools.get("settings"))
    }

    fn parse_allowlist(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_domain_allowed(domain: &str, allowlist: &[String], default_deny: bool) -> bool {
        if allowlist.iter().any(|entry| entry == "*") {
            return true;
        }
        if allowlist.is_empty() {
            return !default_deny;
        }
        allowlist.iter().any(|entry| {
            if entry == domain {
                return true;
            }
            if let Some(suffix) = entry.strip_prefix("*.") {
                return domain.ends_with(suffix);
            }
            false
        })
    }

    fn network_denied_value(domain: &str) -> Value {
        json!({
            "status": "error",
            "message": format!("Network access denied for {}", domain),
        })
    }

    fn extract_query(params: Value) -> Option<String> {
        params
            .get("query")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .filter(|v| !v.trim().is_empty())
    }

    fn format_sources(label: &str, sources: &[String]) -> String {
        if sources.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        out.push_str("\n\n");
        out.push_str(label);
        out.push('\n');
        for (idx, url) in sources.iter().enumerate() {
            out.push_str(&format!("[{}] {}\n", idx + 1, url));
        }
        out.trim_end().to_string()
    }

    async fn search_openai(&self, query: &str, state: &SearchInternetState) -> Result<Value> {
        if !Self::is_domain_allowed("api.openai.com", &state.network_allow, state.default_deny) {
            return Ok(Self::network_denied_value("api.openai.com"));
        }
        let api_key = match &state.api_key {
            Some(key) if !key.trim().is_empty() => key.clone(),
            _ => {
                return Ok(json!({
                    "status": "error",
                    "message": "API key not configured",
                }))
            }
        };

        let payload = json!({
            "model": state.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful assistant that searches the internet for current information."
                },
                {
                    "role": "user",
                    "content": query
                }
            ]
        });

        let response = Client::new()
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                return Ok(json!({
                    "status": "error",
                    "message": "OpenAI API error",
                    "details": err.to_string(),
                }))
            }
        };

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Ok(json!({
                "status": "error",
                "message": format!("Failed to search: {}", status),
                "details": text,
            }));
        }

        let data: Value = response.json().await.unwrap_or(Value::Null);
        let content = data
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("message"))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(json!({
            "status": "success",
            "result": content,
            "model_used": state.model,
        }))
    }

    async fn search_perplexity(&self, query: &str, state: &SearchInternetState) -> Result<Value> {
        if !Self::is_domain_allowed(
            "api.perplexity.ai",
            &state.network_allow,
            state.default_deny,
        ) {
            return Ok(Self::network_denied_value("api.perplexity.ai"));
        }
        let api_key = match &state.api_key {
            Some(key) if !key.trim().is_empty() => key.clone(),
            _ => {
                return Ok(json!({
                    "status": "error",
                    "message": "API key not configured",
                }))
            }
        };

        let system_content = if state.citations {
            "You search the Internet for current information. Include detailed information with citations like [1], [2], etc."
        } else {
            "You search the Internet for current information. Provide a comprehensive answer without citations or source references."
        };

        let payload = json!({
            "model": state.model,
            "messages": [
                {"role": "system", "content": system_content},
                {"role": "user", "content": query}
            ]
        });

        let response = Client::new()
            .post("https://api.perplexity.ai/chat/completions")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                return Ok(json!({
                    "status": "error",
                    "message": "Perplexity API error",
                    "details": err.to_string(),
                }))
            }
        };

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Ok(json!({
                "status": "error",
                "message": format!("Failed to search: {}", status),
                "details": text,
            }));
        }

        let data: Value = response.json().await.unwrap_or(Value::Null);
        let mut content = data
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("message"))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if state.citations {
            if let Some(split) = content.split("Sources:").next() {
                content = split.trim().to_string();
            }
            let citations: Vec<String> = data
                .get("citations")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            if let Some(url) = item.as_str() {
                                Some(url.to_string())
                            } else {
                                item.get("url")
                                    .and_then(|u| u.as_str())
                                    .map(|u| u.to_string())
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let sources = Self::format_sources("**Sources:**", &citations);
            content.push_str(&sources);
        }

        Ok(json!({
            "status": "success",
            "result": content,
            "model_used": state.model,
        }))
    }

    async fn search_grok(&self, query: &str, state: &SearchInternetState) -> Result<Value> {
        if !Self::is_domain_allowed("api.x.ai", &state.network_allow, state.default_deny) {
            return Ok(Self::network_denied_value("api.x.ai"));
        }
        let api_key = match &state.api_key {
            Some(key) if !key.trim().is_empty() => key.clone(),
            _ => {
                return Ok(json!({
                    "status": "error",
                    "message": "API key not configured",
                }))
            }
        };

        let mut tools = Vec::new();
        if state.grok_web_search {
            tools.push(json!({"type": "web_search"}));
        }
        if state.grok_x_search {
            tools.push(json!({"type": "x_search"}));
        }
        if tools.is_empty() {
            tools.push(json!({"type": "web_search"}));
        }

        let payload = json!({
            "model": state.model,
            "input": [
                {"role": "user", "content": query}
            ],
            "tools": tools,
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(state.grok_timeout))
            .build();
        let client = match client {
            Ok(client) => client,
            Err(err) => {
                return Ok(json!({
                    "status": "error",
                    "message": "Grok client error",
                    "details": err.to_string(),
                }))
            }
        };

        let response = client
            .post("https://api.x.ai/v1/responses")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;

        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                return Ok(json!({
                    "status": "error",
                    "message": "Grok API error",
                    "details": err.to_string(),
                }))
            }
        };

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Ok(json!({
                "status": "error",
                "message": format!("Failed to search: {}", status),
                "details": text,
            }));
        }

        let data: Value = response.json().await.unwrap_or(Value::Null);
        let mut content = String::new();
        let mut sources = Vec::new();
        if let Some(items) = data.get("output").and_then(|v| v.as_array()) {
            for item in items {
                if item.get("type").and_then(|v| v.as_str()) == Some("message") {
                    if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                        for part in parts {
                            if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    content = text.to_string();
                                }
                                if let Some(annotations) =
                                    part.get("annotations").and_then(|v| v.as_array())
                                {
                                    for annotation in annotations {
                                        if annotation.get("type").and_then(|v| v.as_str())
                                            == Some("url_citation")
                                        {
                                            if let Some(url) =
                                                annotation.get("url").and_then(|v| v.as_str())
                                            {
                                                if !sources.contains(&url.to_string()) {
                                                    sources.push(url.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                    break;
                }
            }
        }

        if state.citations {
            let formatted = Self::format_sources("**Sources:**", &sources);
            content.push_str(&formatted);
        }

        Ok(json!({
            "status": "success",
            "result": content,
            "model_used": state.model,
        }))
    }
}

impl Default for SearchInternetTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SearchInternetTool {
    fn name(&self) -> &str {
        "search_internet"
    }

    fn description(&self) -> &str {
        "Search the internet for current information using Perplexity, OpenAI, or Grok."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query text"
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn required_secrets_for_config(&self, config: &Value) -> Vec<ToolSecret> {
        let provider = Self::get_tool_config(config)
            .and_then(|cfg| cfg.get("provider"))
            .and_then(|v| v.as_str())
            .unwrap_or("openai");
        match provider {
            "perplexity" => vec![ToolSecret::new(
                "search_internet_perplexity_api_key",
                "Perplexity API key",
            )],
            "grok" => vec![ToolSecret::new(
                "search_internet_grok_api_key",
                "Grok API key",
            )],
            _ => vec![ToolSecret::new(
                "search_internet_openai_api_key",
                "OpenAI API key (for search_internet)",
            )],
        }
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| {
            crate::error::ButterflyBotError::Runtime("Failed to lock tool state".to_string())
        })?;
        let mut model_set_from_config = false;

        if let Some(settings) = Self::get_settings_config(config) {
            if let Some(perms) = settings.get("permissions") {
                if let Some(default_deny) = perms.get("default_deny").and_then(|v| v.as_bool()) {
                    state.default_deny = default_deny;
                }
                if let Some(allow) = perms.get("network_allow") {
                    state.network_allow = Self::parse_allowlist(allow);
                }
            }
        }

        if let Some(tool_cfg) = Self::get_tool_config(config) {
            if let Some(perms) = tool_cfg.get("permissions") {
                if let Some(allow) = perms.get("network_allow") {
                    state.network_allow = Self::parse_allowlist(allow);
                }
            }
            if let Some(api_key) = tool_cfg.get("api_key").and_then(|v| v.as_str()) {
                state.api_key = Some(api_key.to_string());
            }
            if let Some(provider) = tool_cfg.get("provider").and_then(|v| v.as_str()) {
                state.provider = provider.to_string();
            }
            if let Some(model) = tool_cfg.get("model").and_then(|v| v.as_str()) {
                state.model = model.to_string();
                model_set_from_config = true;
            }
            if let Some(citations) = tool_cfg.get("citations").and_then(|v| v.as_bool()) {
                state.citations = citations;
            }
            if let Some(web) = tool_cfg.get("grok_web_search").and_then(|v| v.as_bool()) {
                state.grok_web_search = web;
            }
            if let Some(x_search) = tool_cfg.get("grok_x_search").and_then(|v| v.as_bool()) {
                state.grok_x_search = x_search;
            }
            if let Some(timeout) = tool_cfg.get("grok_timeout").and_then(|v| v.as_u64()) {
                state.grok_timeout = timeout;
            }
        }

        if state.api_key.is_none() {
            if let Some(openai_key) = config
                .get("openai")
                .and_then(|v| v.get("api_key"))
                .and_then(|v| v.as_str())
            {
                if !openai_key.trim().is_empty() {
                    state.api_key = Some(openai_key.to_string());
                }
            }
        }

        Self::set_defaults(&mut state, model_set_from_config);
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let query = match Self::extract_query(params) {
            Some(query) => query,
            None => {
                return Ok(json!({
                    "status": "error",
                    "message": "query is required"
                }))
            }
        };

        let mut state = self.snapshot();

        if state.api_key.is_none() {
            let primary = match state.provider.as_str() {
                "perplexity" => "search_internet_perplexity_api_key",
                "grok" => "search_internet_grok_api_key",
                _ => "search_internet_openai_api_key",
            };
            let fallbacks = [
                "search_internet_grok_api_key",
                "search_internet_perplexity_api_key",
                "search_internet_openai_api_key",
            ];

            for name in std::iter::once(primary).chain(fallbacks.into_iter()) {
                if let Some(secret) = vault::get_secret(name)? {
                    if !secret.trim().is_empty() {
                        state.api_key = Some(secret);
                        break;
                    }
                }
            }
        }

        match state.provider.as_str() {
            "perplexity" => self.search_perplexity(&query, &state).await,
            "grok" => self.search_grok(&query, &state).await,
            _ => self.search_openai(&query, &state).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SearchInternetTool;

    #[test]
    fn network_allowlist_allows_wildcard() {
        let allow = vec!["*".to_string()];
        assert!(SearchInternetTool::is_domain_allowed(
            "api.openai.com",
            &allow,
            true
        ));
    }

    #[test]
    fn network_allowlist_allows_suffix() {
        let allow = vec!["*.openai.com".to_string()];
        assert!(SearchInternetTool::is_domain_allowed(
            "api.openai.com",
            &allow,
            true
        ));
        assert!(!SearchInternetTool::is_domain_allowed(
            "api.perplexity.ai",
            &allow,
            true
        ));
    }

    #[test]
    fn network_allowlist_default_deny() {
        let allow = Vec::new();
        assert!(!SearchInternetTool::is_domain_allowed(
            "api.openai.com",
            &allow,
            true
        ));
        assert!(SearchInternetTool::is_domain_allowed(
            "api.openai.com",
            &allow,
            false
        ));
    }
}
