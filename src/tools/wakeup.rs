use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::wakeup::{default_wakeup_db_path, resolve_wakeup_db_path, WakeupStatus, WakeupStore};

pub struct WakeupTool {
    sqlite_path: RwLock<Option<String>>,
    store: RwLock<Option<std::sync::Arc<WakeupStore>>>,
}

impl Default for WakeupTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WakeupTool {
    pub fn new() -> Self {
        Self {
            sqlite_path: RwLock::new(None),
            store: RwLock::new(None),
        }
    }

    async fn get_store(&self) -> Result<std::sync::Arc<WakeupStore>> {
        if let Some(store) = self.store.read().await.as_ref() {
            return Ok(store.clone());
        }
        let path = self
            .sqlite_path
            .read()
            .await
            .clone()
            .unwrap_or_else(default_wakeup_db_path);
        let store = std::sync::Arc::new(WakeupStore::new(path).await?);
        let mut guard = self.store.write().await;
        *guard = Some(store.clone());
        Ok(store)
    }
}

#[async_trait]
impl Tool for WakeupTool {
    fn name(&self) -> &str {
        "wakeup"
    }

    fn description(&self) -> &str {
        "Schedule background wakeups that run the agent with a task prompt at an interval."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "enable", "disable", "delete"]
                },
                "user_id": { "type": "string" },
                "name": { "type": "string" },
                "prompt": { "type": "string" },
                "interval_minutes": { "type": "integer" },
                "status": { "type": "string", "enum": ["enabled", "disabled", "all"] },
                "limit": { "type": "integer" },
                "id": { "type": "integer" }
            },
            "required": ["action", "user_id"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let path = resolve_wakeup_db_path(config);
        let mut guard = self
            .sqlite_path
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("Wakeup tool lock busy".to_string()))?;
        *guard = path;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let user_id = params
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ButterflyBotError::Runtime("Missing user_id".to_string()))?;

        let store = self.get_store().await?;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match action.as_str() {
            "create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing name".to_string()))?;
                let prompt = params
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing prompt".to_string()))?;
                let interval_minutes = params
                    .get("interval_minutes")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| {
                        ButterflyBotError::Runtime("Missing interval_minutes".to_string())
                    })?;
                let item = store
                    .create_task(user_id, name, prompt, interval_minutes)
                    .await?;
                Ok(json!({"status": "ok", "task": item}))
            }
            "list" => {
                let status =
                    WakeupStatus::from_option(params.get("status").and_then(|v| v.as_str()));
                let items = store.list_tasks(user_id, status, limit).await?;
                Ok(json!({"status": "ok", "tasks": items}))
            }
            "enable" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let item = store.set_enabled(id, true).await?;
                Ok(json!({"status": "ok", "task": item}))
            }
            "disable" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let item = store.set_enabled(id, false).await?;
                Ok(json!({"status": "ok", "task": item}))
            }
            "delete" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let deleted = store.delete_task(id).await?;
                Ok(json!({"status": "ok", "deleted": deleted}))
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}
