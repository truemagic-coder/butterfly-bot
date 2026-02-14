use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;

use crate::config_store;
use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::sandbox::{SandboxSettings, WasmRuntime};

#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    agent_tools: RwLock<HashMap<String, HashSet<String>>>,
    config: RwLock<serde_json::Value>,
    audit_log_path: RwLock<Option<String>>,
    sandbox: RwLock<SandboxSettings>,
    wasm_runtime: WasmRuntime,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            agent_tools: RwLock::new(HashMap::new()),
            config: RwLock::new(serde_json::Value::Object(Default::default())),
            audit_log_path: RwLock::new(Some("./data/tool_audit.log".to_string())),
            sandbox: RwLock::new(SandboxSettings::default()),
            wasm_runtime: WasmRuntime,
        }
    }

    pub async fn register_tool(&self, tool: Arc<dyn Tool>) -> bool {
        let config = self.config.read().await.clone();
        if let Err(err) = tool.configure(&config) {
            let _ = err;
            return false;
        }
        let mut tools = self.tools.write().await;
        let name = tool.name().to_string();
        if tools.contains_key(&name) {
            return false;
        }
        tools.insert(name.clone(), tool);
        true
    }

    pub async fn assign_tool_to_agent(&self, agent_name: &str, tool_name: &str) -> bool {
        let tools = self.tools.read().await;
        if !tools.contains_key(tool_name) {
            return false;
        }
        let mut agent_tools = self.agent_tools.write().await;
        agent_tools
            .entry(agent_name.to_string())
            .or_default()
            .insert(tool_name.to_string());
        true
    }

    pub async fn get_tool(&self, tool_name: &str) -> Option<Arc<dyn Tool>> {
        let tools = self.tools.read().await;
        tools.get(tool_name).cloned()
    }

    pub async fn get_agent_tools(&self, agent_name: &str) -> Vec<Arc<dyn Tool>> {
        let agent_tools = self.agent_tools.read().await;
        let tools = self.tools.read().await;
        let names = agent_tools.get(agent_name).cloned().unwrap_or_default();
        names
            .into_iter()
            .filter_map(|name| tools.get(&name).cloned())
            .collect()
    }

    pub async fn list_all_tools(&self) -> Vec<String> {
        let tools = self.tools.read().await;
        tools.keys().cloned().collect()
    }

    pub async fn has_mcp_servers(&self) -> bool {
        let config = self.config.read().await.clone();
        config
            .get("tools")
            .and_then(|tools| tools.get("mcp"))
            .and_then(|mcp| mcp.get("servers"))
            .and_then(|servers| servers.as_array())
            .map(|servers| !servers.is_empty())
            .unwrap_or(false)
    }

    pub async fn configure_all_tools(&self, config: serde_json::Value) -> Result<()> {
        {
            let mut cfg = self.config.write().await;
            *cfg = config.clone();
        }
        {
            let mut sandbox = self.sandbox.write().await;
            *sandbox = SandboxSettings::from_root_config(&config);
        }
        if let Some(settings) = config.get("tools").and_then(|v| v.get("settings")) {
            if let Some(path) = settings
                .get("audit_log_path")
                .and_then(|v| v.as_str())
                .map(|v| v.trim())
            {
                let mut guard = self.audit_log_path.write().await;
                if path.is_empty() {
                    *guard = None;
                } else {
                    *guard = Some(path.to_string());
                }
            }
        }

        let tools = self.tools.read().await;
        for tool in tools.values() {
            tool.configure(&config)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        Ok(())
    }

    pub async fn execute_tool(&self, tool_name: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let tool = {
            let tools = self.tools.read().await;
            tools.get(tool_name).cloned()
        }
        .ok_or_else(|| ButterflyBotError::Runtime(format!("Tool not found: {tool_name}")))?;

        let plan = {
            let sandbox = self.sandbox.read().await;
            sandbox.execution_plan(tool_name)
        };

        let _ = self
            .audit_sandbox_decision(tool_name, plan.runtime.as_str(), &plan.reason)
            .await;

        match plan.runtime {
            crate::sandbox::ToolRuntime::Native => tool.execute(params).await,
            crate::sandbox::ToolRuntime::Wasm => {
                self.wasm_runtime.execute(tool_name, &plan.tool_config, params).await
            }
        }
    }

    pub async fn audit_tool_call(&self, tool_name: &str, status: &str) -> Result<()> {
        let path = self.audit_log_path.read().await.clone();
        let Some(path) = path else {
            return Ok(());
        };
        config_store::ensure_parent_dir(&path)?;

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs();
        let payload = serde_json::json!({
            "timestamp": ts,
            "tool": tool_name,
            "status": status,
        });

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        writeln!(file, "{}", payload).map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(())
    }

    pub async fn audit_sandbox_decision(
        &self,
        tool_name: &str,
        runtime: &str,
        reason: &str,
    ) -> Result<()> {
        let path = self.audit_log_path.read().await.clone();
        let Some(path) = path else {
            return Ok(());
        };
        config_store::ensure_parent_dir(&path)?;

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs();
        let payload = serde_json::json!({
            "timestamp": ts,
            "type": "sandbox_decision",
            "tool": tool_name,
            "runtime": runtime,
            "reason": reason,
        });

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        writeln!(file, "{}", payload).map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(())
    }
}
