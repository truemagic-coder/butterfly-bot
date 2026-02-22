use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;

use crate::config_store;
use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::sandbox::{SandboxSettings, ToolRuntime, WasmRuntime};

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

        let sandbox = self.sandbox.read().await.clone();
        let tools = self.tools.read().await;
        for (tool_name, tool) in tools.iter() {
            let plan = sandbox.execution_plan(tool_name);
            WasmRuntime::validate_module_binary(tool_name, &plan.tool_config)?;
            WasmRuntime::validate_capability_abi(tool_name, &plan.tool_config)?;
            tool.configure(&config)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        Ok(())
    }

    pub async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let tool = {
            let tools = self.tools.read().await;
            tools.get(tool_name).cloned()
        };
        let Some(tool) = tool else {
            return Err(ButterflyBotError::Runtime(format!(
                "Tool not found: {tool_name}"
            )));
        };

        let plan = {
            let sandbox = self.sandbox.read().await;
            sandbox.execution_plan(tool_name)
        };

        let _ = self
            .audit_sandbox_decision(tool_name, plan.runtime.as_str(), &plan.reason)
            .await;

        let original_params = params.clone();
        let wasm_result = self
            .wasm_runtime
            .execute(tool_name, &plan.tool_config, params)
            .await?;

        if tool_name == "solana" {
            let is_invalid_args = wasm_result.get("status").and_then(|value| value.as_str())
                == Some("error")
                && wasm_result.get("code").and_then(|value| value.as_str()) == Some("invalid_args");

            let has_spl_transfer_args = original_params
                .get("mint")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
                && original_params
                    .get("amount_atomic")
                    .and_then(|value| value.as_u64())
                    .is_some();

            if is_invalid_args && has_spl_transfer_args {
                return tool.execute(original_params).await;
            }
        }

        let is_stub = wasm_result
            .get("stub")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if !is_stub {
            if wasm_result.get("status").and_then(|value| value.as_str()) == Some("capability_call")
            {
                return self
                    .execute_capability_call(tool_name, &tool, &plan.tool_config, &wasm_result)
                    .await;
            }

            if wasm_result.get("status").and_then(|value| value.as_str()) == Some("host_call") {
                return Err(ButterflyBotError::Runtime(format!(
                    "Tool '{}' attempted deprecated wasm host_call fallback",
                    tool_name
                )));
            }

            return Ok(wasm_result);
        }

        Err(ButterflyBotError::Runtime(format!(
            "WASM tool '{tool_name}' returned a stub response. Install a real WASM implementation for this tool (current module is a placeholder)."
        )))
    }

    async fn execute_capability_call(
        &self,
        tool_name: &str,
        tool: &Arc<dyn Tool>,
        tool_config: &crate::sandbox::ToolSandboxConfig,
        wasm_result: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let call = wasm_result.get("capability_call").ok_or_else(|| {
            ButterflyBotError::Runtime(
                "WASM capability_call missing `capability_call` payload".to_string(),
            )
        })?;

        let requested_abi = wasm_result
            .get("abi_version")
            .and_then(|value| value.as_u64())
            .unwrap_or(WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION as u64)
            as u32;
        if requested_abi != WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION {
            return Ok(serde_json::json!({
                "status": "error",
                "code": "invalid_args",
                "error": format!(
                    "Unsupported capability ABI version {} (supported: {})",
                    requested_abi,
                    WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION
                )
            }));
        }

        let capability = call
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                ButterflyBotError::Runtime("WASM capability_call missing `name`".to_string())
            })?;

        if !tool_config.is_capability_allowed(capability) {
            return Ok(serde_json::json!({
                "status": "error",
                "code": "forbidden",
                "error": format!(
                    "Capability '{}' is not allowed for tool '{}'",
                    capability,
                    tool_name
                )
            }));
        }

        let args = call
            .get("args")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        let response = match capability {
            "clock.now_unix" => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
                    .as_secs() as i64;
                serde_json::json!({
                    "status": "ok",
                    "abi_version": WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION,
                    "capability_result": {
                        "name": capability,
                        "result": { "unix": now }
                    }
                })
            }
            "log.emit" => {
                let level = args.get("level").and_then(|v| v.as_str()).unwrap_or("info");
                let event = args
                    .get("event")
                    .and_then(|v| v.as_str())
                    .unwrap_or("wasm_capability_log_emit");
                let reason = format!("{level}:{event}");
                let _ = self
                    .audit_sandbox_decision(tool_name, "wasm_capability_log", &reason)
                    .await;
                serde_json::json!({
                    "status": "ok",
                    "abi_version": WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION,
                    "capability_result": {
                        "name": capability,
                        "result": { "logged": true }
                    }
                })
            }
            "kv.sqlite.todo.create" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    let user_id = Self::require_str(args, "user_id")?;
                    let title = Self::require_str(args, "title")?;
                    let notes = args.get("notes").and_then(|v| v.as_str());
                    Ok(serde_json::json!({
                        "action": "create",
                        "user_id": user_id,
                        "title": title,
                        "notes": notes
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.list" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    let user_id = Self::require_str(args, "user_id")?;
                    let status = args
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("open");
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);
                    Ok(serde_json::json!({
                        "action": "list",
                        "user_id": user_id,
                        "status": status,
                        "limit": limit
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.create_many" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    let user_id = Self::require_str(args, "user_id")?;
                    let items = args
                        .get("items")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| {
                            ButterflyBotError::Runtime("capability args missing items".to_string())
                        })?
                        .clone();
                    Ok(serde_json::json!({
                        "action": "create_many",
                        "user_id": user_id,
                        "items": items
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.complete" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "complete",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.reopen" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "reopen",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.delete" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "delete",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.clear" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "clear",
                        "user_id": Self::require_str(args, "user_id")?,
                        "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("open")
                    }))
                })
                .await?
            }
            "kv.sqlite.todo.reorder" => {
                self.execute_tool_capability(tool_name, tool, "todo", capability, &args, |args| {
                    let user_id = Self::require_str(args, "user_id")?;
                    let ordered_ids = args
                        .get("ordered_ids")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| {
                            ButterflyBotError::Runtime(
                                "capability args missing ordered_ids".to_string(),
                            )
                        })?
                        .clone();
                    Ok(serde_json::json!({
                        "action": "reorder",
                        "user_id": user_id,
                        "ordered_ids": ordered_ids
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.schedule" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "schedule",
                        "user_id": Self::require_str(args, "user_id")?,
                        "name": Self::require_str(args, "name")?,
                        "prompt": Self::require_str(args, "prompt")?,
                        "run_at": Self::require_i64(args, "run_at")?,
                        "interval_minutes": args.get("interval_minutes").and_then(|v| v.as_i64())
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.list" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "list",
                        "user_id": Self::require_str(args, "user_id")?,
                        "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("all"),
                        "limit": args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50)
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.enable" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "enable",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.disable" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "disable",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.delete" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "delete",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.tasks.clear" => {
                self.execute_tool_capability(tool_name, tool, "tasks", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "clear",
                        "user_id": Self::require_str(args, "user_id")?,
                        "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("all")
                    }))
                })
                .await?
            }
            "kv.sqlite.reminders.create" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "create",
                            "user_id": Self::require_str(args, "user_id")?,
                            "title": Self::require_str(args, "title")?,
                            "due_at": args.get("due_at").and_then(|v| v.as_i64()),
                            "delay_seconds": args.get("delay_seconds").and_then(|v| v.as_i64()),
                            "in_seconds": args.get("in_seconds").and_then(|v| v.as_i64())
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.reminders.list" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "list",
                            "user_id": Self::require_str(args, "user_id")?,
                            "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("open"),
                            "limit": args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20)
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.reminders.complete" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "complete",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.reminders.delete" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "delete",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.reminders.snooze" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "snooze",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?,
                            "due_at": args.get("due_at").and_then(|v| v.as_i64()),
                            "delay_seconds": args.get("delay_seconds").and_then(|v| v.as_i64()),
                            "in_seconds": args.get("in_seconds").and_then(|v| v.as_i64())
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.reminders.clear" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "reminders",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "clear",
                            "user_id": Self::require_str(args, "user_id")?,
                            "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("open")
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.create" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "create",
                            "user_id": Self::require_str(args, "user_id")?,
                            "title": Self::require_str(args, "title")?,
                            "goal": Self::require_str(args, "goal")?,
                            "steps": args.get("steps").cloned(),
                            "status": args.get("status").and_then(|v| v.as_str())
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.list" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "list",
                            "user_id": Self::require_str(args, "user_id")?,
                            "limit": args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20)
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.get" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "get",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.update" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "update",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?,
                            "title": args.get("title").and_then(|v| v.as_str()),
                            "goal": args.get("goal").and_then(|v| v.as_str()),
                            "steps": args.get("steps").cloned(),
                            "status": args.get("status").and_then(|v| v.as_str())
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.delete" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "delete",
                            "user_id": Self::require_str(args, "user_id")?,
                            "id": Self::require_i64(args, "id")?
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.planning.clear" => {
                self.execute_tool_capability(
                    tool_name,
                    tool,
                    "planning",
                    capability,
                    &args,
                    |args| {
                        Ok(serde_json::json!({
                            "action": "clear",
                            "user_id": Self::require_str(args, "user_id")?
                        }))
                    },
                )
                .await?
            }
            "kv.sqlite.wakeup.create" => {
                self.execute_tool_capability(tool_name, tool, "wakeup", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "create",
                        "user_id": Self::require_str(args, "user_id")?,
                        "name": Self::require_str(args, "name")?,
                        "prompt": Self::require_str(args, "prompt")?,
                        "interval_minutes": Self::require_i64(args, "interval_minutes")?
                    }))
                })
                .await?
            }
            "kv.sqlite.wakeup.list" => {
                self.execute_tool_capability(tool_name, tool, "wakeup", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "list",
                        "user_id": Self::require_str(args, "user_id")?,
                        "status": args.get("status").and_then(|v| v.as_str()).unwrap_or("all"),
                        "limit": args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20)
                    }))
                })
                .await?
            }
            "kv.sqlite.wakeup.enable" => {
                self.execute_tool_capability(tool_name, tool, "wakeup", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "enable",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.wakeup.disable" => {
                self.execute_tool_capability(tool_name, tool, "wakeup", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "disable",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "kv.sqlite.wakeup.delete" => {
                self.execute_tool_capability(tool_name, tool, "wakeup", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "delete",
                        "user_id": Self::require_str(args, "user_id")?,
                        "id": Self::require_i64(args, "id")?
                    }))
                })
                .await?
            }
            "http.request" => {
                self.execute_cross_tool_capability(
                    capability,
                    "http_call",
                    serde_json::json!({
                        "method": Self::require_str(&args, "method")?,
                        "server": args.get("server").and_then(|v| v.as_str()),
                        "url": args.get("url").and_then(|v| v.as_str()),
                        "endpoint": args.get("endpoint").and_then(|v| v.as_str()),
                        "headers": args.get("headers").cloned(),
                        "query": args.get("query").cloned(),
                        "body": args.get("body").and_then(|v| v.as_str()),
                        "json": args.get("json").cloned(),
                        "timeout_seconds": args.get("timeout_seconds").and_then(|v| v.as_u64())
                    }),
                )
                .await?
            }
            "coding.generate" => {
                self.execute_cross_tool_capability(
                    capability,
                    "coding",
                    serde_json::json!({
                        "prompt": Self::require_str(&args, "prompt")?,
                        "system_prompt": args.get("system_prompt").and_then(|v| v.as_str())
                    }),
                )
                .await?
            }
            "mcp.list_tools" => {
                self.execute_cross_tool_capability(
                    capability,
                    "mcp",
                    serde_json::json!({
                        "action": "list_tools",
                        "server": args.get("server").and_then(|v| v.as_str())
                    }),
                )
                .await?
            }
            "mcp.call" => {
                let tool_name = args
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .or_else(|| args.get("method").and_then(|v| v.as_str()))
                    .ok_or_else(|| {
                        ButterflyBotError::Runtime(
                            "capability args missing tool/method".to_string(),
                        )
                    })?;
                let arguments = args
                    .get("arguments")
                    .cloned()
                    .or_else(|| args.get("payload").cloned());

                self.execute_cross_tool_capability(
                    capability,
                    "mcp",
                    serde_json::json!({
                        "action": "call_tool",
                        "server": args.get("server").and_then(|v| v.as_str()),
                        "tool": tool_name,
                        "arguments": arguments
                    }),
                )
                .await?
            }
            "github.list_tools" => {
                self.execute_cross_tool_capability(
                    capability,
                    "github",
                    serde_json::json!({
                        "action": "list_tools"
                    }),
                )
                .await?
            }
            "github.call_tool" => {
                self.execute_cross_tool_capability(
                    capability,
                    "github",
                    serde_json::json!({
                        "action": "call_tool",
                        "tool": Self::require_str(&args, "tool")?,
                        "arguments": args.get("arguments").cloned()
                    }),
                )
                .await?
            }
            "zapier.list_tools" => {
                self.execute_cross_tool_capability(
                    capability,
                    "zapier",
                    serde_json::json!({
                        "action": "list_tools"
                    }),
                )
                .await?
            }
            "zapier.call_tool" => {
                self.execute_cross_tool_capability(
                    capability,
                    "zapier",
                    serde_json::json!({
                        "action": "call_tool",
                        "tool": Self::require_str(&args, "tool")?,
                        "arguments": args.get("arguments").cloned()
                    }),
                )
                .await?
            }
            "search.internet" => {
                self.execute_cross_tool_capability(
                    capability,
                    "search_internet",
                    serde_json::json!({
                        "query": Self::require_str(&args, "query")?
                    }),
                )
                .await?
            }
            "solana.wallet" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "wallet",
                        "user_id": Self::require_str(args, "user_id")?,
                        "actor": args.get("actor").and_then(|v| v.as_str())
                    }))
                })
                .await?
            }
            "solana.balance" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    let address = args.get("address").and_then(|v| v.as_str());
                    let user_id = args.get("user_id").and_then(|v| v.as_str());
                    if address.is_none() && user_id.is_none() {
                        return Err(ButterflyBotError::Runtime(
                            "capability args missing address or user_id".to_string(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "action": "balance",
                        "address": address,
                        "user_id": user_id,
                        "actor": args.get("actor").and_then(|v| v.as_str())
                    }))
                })
                .await?
            }
            "solana.transfer" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    let lamports = args.get("lamports").and_then(|v| v.as_u64());
                    let mint = args
                        .get("mint")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string);
                    let amount_atomic = args.get("amount_atomic").and_then(|v| v.as_u64());

                    if lamports.is_none() && (mint.is_none() || amount_atomic.is_none()) {
                        return Err(ButterflyBotError::Runtime(
                            "capability args missing lamports (or mint+amount_atomic)".to_string(),
                        ));
                    }

                    let mut payload = serde_json::Map::new();
                    payload.insert(
                        "action".to_string(),
                        serde_json::Value::String("transfer".to_string()),
                    );
                    payload.insert(
                        "user_id".to_string(),
                        serde_json::Value::String(Self::require_str(args, "user_id")?.to_string()),
                    );
                    payload.insert(
                        "to".to_string(),
                        serde_json::Value::String(Self::require_str(args, "to")?.to_string()),
                    );
                    if let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) {
                        payload.insert(
                            "request_id".to_string(),
                            serde_json::Value::String(request_id.to_string()),
                        );
                    }
                    if let Some(actor) = args.get("actor").and_then(|v| v.as_str()) {
                        payload.insert(
                            "actor".to_string(),
                            serde_json::Value::String(actor.to_string()),
                        );
                    }
                    if let Some(lamports) = lamports {
                        payload.insert("lamports".to_string(), serde_json::Value::from(lamports));
                    }
                    if let Some(mint) = mint {
                        payload.insert("mint".to_string(), serde_json::Value::String(mint));
                    }
                    if let Some(amount_atomic) = amount_atomic {
                        payload.insert(
                            "amount_atomic".to_string(),
                            serde_json::Value::from(amount_atomic),
                        );
                    }
                    Ok(serde_json::Value::Object(payload))
                })
                .await?
            }
            "solana.simulate_transfer" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    let lamports = args.get("lamports").and_then(|v| v.as_u64());
                    let mint = args
                        .get("mint")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string);
                    let amount_atomic = args.get("amount_atomic").and_then(|v| v.as_u64());

                    if lamports.is_none() && (mint.is_none() || amount_atomic.is_none()) {
                        return Err(ButterflyBotError::Runtime(
                            "capability args missing lamports (or mint+amount_atomic)".to_string(),
                        ));
                    }

                    let mut payload = serde_json::Map::new();
                    payload.insert(
                        "action".to_string(),
                        serde_json::Value::String("simulate_transfer".to_string()),
                    );
                    payload.insert(
                        "user_id".to_string(),
                        serde_json::Value::String(Self::require_str(args, "user_id")?.to_string()),
                    );
                    payload.insert(
                        "to".to_string(),
                        serde_json::Value::String(Self::require_str(args, "to")?.to_string()),
                    );
                    if let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) {
                        payload.insert(
                            "request_id".to_string(),
                            serde_json::Value::String(request_id.to_string()),
                        );
                    }
                    if let Some(actor) = args.get("actor").and_then(|v| v.as_str()) {
                        payload.insert(
                            "actor".to_string(),
                            serde_json::Value::String(actor.to_string()),
                        );
                    }
                    if let Some(lamports) = lamports {
                        payload.insert("lamports".to_string(), serde_json::Value::from(lamports));
                    }
                    if let Some(mint) = mint {
                        payload.insert("mint".to_string(), serde_json::Value::String(mint));
                    }
                    if let Some(amount_atomic) = amount_atomic {
                        payload.insert(
                            "amount_atomic".to_string(),
                            serde_json::Value::from(amount_atomic),
                        );
                    }
                    Ok(serde_json::Value::Object(payload))
                })
                .await?
            }
            "solana.tx_status" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    Ok(serde_json::json!({
                        "action": "tx_status",
                        "signature": Self::require_str(args, "signature")?
                    }))
                })
                .await?
            }
            "solana.tx_history" => {
                self.execute_tool_capability(tool_name, tool, "solana", capability, &args, |args| {
                    let address = args.get("address").and_then(|v| v.as_str());
                    let user_id = args.get("user_id").and_then(|v| v.as_str());
                    if address.is_none() && user_id.is_none() {
                        return Err(ButterflyBotError::Runtime(
                            "capability args missing address or user_id".to_string(),
                        ));
                    }
                    Ok(serde_json::json!({
                        "action": "tx_history",
                        "address": address,
                        "user_id": user_id,
                        "actor": args.get("actor").and_then(|v| v.as_str()),
                        "limit": args.get("limit").and_then(|v| v.as_u64())
                    }))
                })
                .await?
            }
            "secrets.get" => {
                let secret_name = Self::require_str(&args, "name")?;
                let scoped = format!("secrets.get.{secret_name}");
                if !tool_config.is_capability_allowed(&scoped)
                    && !tool_config.is_capability_allowed("secrets.get")
                {
                    serde_json::json!({
                        "status": "error",
                        "code": "forbidden",
                        "error": format!(
                            "Secret '{}' is not allowlisted for tool '{}'",
                            secret_name,
                            tool_name
                        )
                    })
                } else {
                    let secret_value = crate::vault::get_secret(secret_name)?;
                    serde_json::json!({
                        "status": "ok",
                        "abi_version": WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION,
                        "capability_result": {
                            "name": capability,
                            "result": {
                                "name": secret_name,
                                "found": secret_value.is_some(),
                                "value": secret_value
                            }
                        }
                    })
                }
            }
            _ => {
                serde_json::json!({
                    "status": "error",
                    "code": "internal",
                    "error": format!(
                        "Capability '{}' requested by wasm tool '{}' but host bridge for this capability is not implemented yet",
                        capability,
                        tool_name
                    )
                })
            }
        };

        let _ = self
            .audit_sandbox_decision(tool_name, "wasm_capability_call", capability)
            .await;

        Ok(response)
    }

    async fn execute_tool_capability<F>(
        &self,
        tool_name: &str,
        tool: &Arc<dyn Tool>,
        expected_tool_name: &str,
        capability: &str,
        args: &serde_json::Value,
        map_args: F,
    ) -> Result<serde_json::Value>
    where
        F: FnOnce(&serde_json::Value) -> Result<serde_json::Value>,
    {
        if tool_name != expected_tool_name {
            return Ok(serde_json::json!({
                "status": "error",
                "code": "forbidden",
                "error": format!(
                    "Capability '{}' is only valid for tool '{}'",
                    capability,
                    expected_tool_name
                )
            }));
        }

        let mapped_args = map_args(args)?;
        let tool_result = tool.execute(mapped_args).await?;

        Ok(serde_json::json!({
            "status": "ok",
            "abi_version": WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION,
            "capability_result": {
                "name": capability,
                "result": tool_result
            }
        }))
    }

    async fn execute_cross_tool_capability(
        &self,
        capability: &str,
        target_tool_name: &str,
        mapped_args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let target_tool = self.get_tool(target_tool_name).await.ok_or_else(|| {
            ButterflyBotError::Runtime(format!(
                "Capability '{}' requires tool '{}' to be registered",
                capability, target_tool_name
            ))
        })?;

        let tool_result = target_tool.execute(mapped_args).await?;
        Ok(serde_json::json!({
            "status": "ok",
            "abi_version": WasmRuntime::SUPPORTED_CAPABILITY_ABI_VERSION,
            "capability_result": {
                "name": capability,
                "result": tool_result
            }
        }))
    }

    fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str> {
        args.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| ButterflyBotError::Runtime(format!("capability args missing {key}")))
    }

    fn require_i64(args: &serde_json::Value, key: &str) -> Result<i64> {
        args.get(key)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| ButterflyBotError::Runtime(format!("capability args missing {key}")))
    }

    pub async fn resolved_runtime_for_tool(&self, tool_name: &str) -> ToolRuntime {
        let sandbox = self.sandbox.read().await;
        sandbox.execution_plan(tool_name).runtime
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::ToolRegistry;
    use crate::error::Result;
    use crate::interfaces::plugins::Tool;
    use crate::sandbox::ToolSandboxConfig;

    struct EchoTool {
        tool_name: String,
    }

    fn echo_tool(name: &str) -> Arc<dyn Tool> {
        Arc::new(EchoTool {
            tool_name: name.to_string(),
        })
    }

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            self.tool_name.as_str()
        }

        fn description(&self) -> &str {
            "echo"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type":"object"})
        }

        fn configure(&self, _config: &serde_json::Value) -> Result<()> {
            Ok(())
        }

        async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
            Ok(serde_json::json!({"echo": params}))
        }
    }

    #[tokio::test]
    async fn capability_call_rejects_disallowed_capability() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("todo");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["clock.now_unix".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.todo.create",
                        "args": {}
                    }
                }),
            )
            .await
            .expect("capability call should return deterministic error response");

        assert_eq!(result.get("status").and_then(|v| v.as_str()), Some("error"));
        assert_eq!(
            result.get("code").and_then(|v| v.as_str()),
            Some("forbidden")
        );
    }

    #[tokio::test]
    async fn capability_call_supports_clock_now_unix() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("todo");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["clock.now_unix".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "clock.now_unix",
                        "args": {}
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            result
                .get("capability_result")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("clock.now_unix")
        );
        assert!(result
            .get("capability_result")
            .and_then(|v| v.get("result"))
            .and_then(|v| v.get("unix"))
            .and_then(|v| v.as_i64())
            .is_some());
    }

    #[tokio::test]
    async fn capability_call_supports_todo_create_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("todo");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.todo.create".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.todo.create",
                        "args": {
                            "user_id": "u1",
                            "title": "hello"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            result
                .get("capability_result")
                .and_then(|v| v.get("result"))
                .and_then(|v| v.get("echo"))
                .and_then(|v| v.get("action"))
                .and_then(|v| v.as_str()),
            Some("create")
        );
    }

    #[tokio::test]
    async fn capability_call_supports_todo_reorder_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("todo");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.todo.reorder".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.todo.reorder",
                        "args": {
                            "user_id": "u1",
                            "ordered_ids": [1,2,3]
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "reorder"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_tasks_schedule_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("tasks");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.tasks.schedule".to_string()];

        let result = registry
            .execute_capability_call(
                "tasks",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.tasks.schedule",
                        "args": {
                            "user_id": "u1",
                            "name": "run",
                            "prompt": "do it",
                            "run_at": 1730000000
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "schedule"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_reminders_create_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("reminders");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.reminders.create".to_string()];

        let result = registry
            .execute_capability_call(
                "reminders",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.reminders.create",
                        "args": {
                            "user_id": "u1",
                            "title": "ping"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "create"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_planning_create_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("planning");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.planning.create".to_string()];

        let result = registry
            .execute_capability_call(
                "planning",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.planning.create",
                        "args": {
                            "user_id": "u1",
                            "title": "plan",
                            "goal": "ship"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "create"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_wakeup_create_bridge() {
        let registry = ToolRegistry::new();
        let tool = echo_tool("wakeup");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["kv.sqlite.wakeup.create".to_string()];

        let result = registry
            .execute_capability_call(
                "wakeup",
                &tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "kv.sqlite.wakeup.create",
                        "args": {
                            "user_id": "u1",
                            "name": "wake",
                            "prompt": "check",
                            "interval_minutes": 30
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "create"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_http_request_bridge() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let http_tool = echo_tool("http_call");
        assert!(registry.register_tool(http_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["http.request".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "http.request",
                        "args": {
                            "method": "GET",
                            "url": "https://example.com"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["method"],
            "GET"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_mcp_call_bridge() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let mcp_tool = echo_tool("mcp");
        assert!(registry.register_tool(mcp_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["mcp.call".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "mcp.call",
                        "args": {
                            "server": "local",
                            "method": "search",
                            "payload": {"q": "rust"}
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "call_tool"
        );
        assert_eq!(
            result["capability_result"]["result"]["echo"]["tool"],
            "search"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_coding_generate_bridge() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let coding_tool = echo_tool("coding");
        assert!(registry.register_tool(coding_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["coding.generate".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "coding.generate",
                        "args": {
                            "prompt": "write a test"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["prompt"],
            "write a test"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_mcp_list_tools_bridge() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let mcp_tool = echo_tool("mcp");
        assert!(registry.register_tool(mcp_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["mcp.list_tools".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "mcp.list_tools",
                        "args": {
                            "server": "local"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["action"],
            "list_tools"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_github_bridges() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let github_tool = echo_tool("github");
        assert!(registry.register_tool(github_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec![
            "github.list_tools".to_string(),
            "github.call_tool".to_string(),
        ];

        let list_result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "github.list_tools",
                        "args": {}
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(list_result["status"], "ok");
        assert_eq!(
            list_result["capability_result"]["result"]["echo"]["action"],
            "list_tools"
        );

        let call_result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "github.call_tool",
                        "args": {
                            "tool": "search",
                            "arguments": {"q": "rust"}
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(call_result["status"], "ok");
        assert_eq!(
            call_result["capability_result"]["result"]["echo"]["action"],
            "call_tool"
        );
        assert_eq!(
            call_result["capability_result"]["result"]["echo"]["tool"],
            "search"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_search_internet_bridge() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let search_tool = echo_tool("search_internet");
        assert!(registry.register_tool(search_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["search.internet".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "search.internet",
                        "args": {
                            "query": "latest rust release"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(result["status"], "ok");
        assert_eq!(
            result["capability_result"]["result"]["echo"]["query"],
            "latest rust release"
        );
    }

    #[tokio::test]
    async fn capability_call_supports_zapier_bridges() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let zapier_tool = echo_tool("zapier");
        assert!(registry.register_tool(zapier_tool).await);

        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec![
            "zapier.list_tools".to_string(),
            "zapier.call_tool".to_string(),
        ];

        let list_result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "zapier.list_tools",
                        "args": {}
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(list_result["status"], "ok");
        assert_eq!(
            list_result["capability_result"]["result"]["echo"]["action"],
            "list_tools"
        );

        let call_result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "zapier.call_tool",
                        "args": {
                            "tool": "find_zaps",
                            "arguments": {"q": "calendar"}
                        }
                    }
                }),
            )
            .await
            .expect("capability call should succeed");

        assert_eq!(call_result["status"], "ok");
        assert_eq!(
            call_result["capability_result"]["result"]["echo"]["action"],
            "call_tool"
        );
        assert_eq!(
            call_result["capability_result"]["result"]["echo"]["tool"],
            "find_zaps"
        );
    }

    #[tokio::test]
    async fn capability_call_scoped_secret_requires_matching_allowlist() {
        let registry = ToolRegistry::new();
        let caller_tool = echo_tool("todo");
        let mut cfg = ToolSandboxConfig::default();
        cfg.capabilities.allow = vec!["secrets.get.allowed_secret".to_string()];

        let result = registry
            .execute_capability_call(
                "todo",
                &caller_tool,
                &cfg,
                &serde_json::json!({
                    "status": "capability_call",
                    "abi_version": 1,
                    "capability_call": {
                        "name": "secrets.get",
                        "args": {
                            "name": "other_secret"
                        }
                    }
                }),
            )
            .await
            .expect("capability call should return deterministic error");

        assert_eq!(result["status"], "error");
        assert_eq!(result["code"], "forbidden");
    }
}
