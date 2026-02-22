use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::todo::{default_todo_db_path, resolve_todo_db_path, TodoStatus, TodoStore};

pub struct TodoTool {
    sqlite_path: RwLock<Option<String>>,
    store: RwLock<Option<std::sync::Arc<TodoStore>>>,
}

impl Default for TodoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoTool {
    pub fn new() -> Self {
        Self {
            sqlite_path: RwLock::new(None),
            store: RwLock::new(None),
        }
    }

    async fn get_store(&self) -> Result<std::sync::Arc<TodoStore>> {
        if let Some(store) = self.store.read().await.as_ref() {
            return Ok(store.clone());
        }
        let path = self
            .sqlite_path
            .read()
            .await
            .clone()
            .unwrap_or_else(default_todo_db_path);
        let store = std::sync::Arc::new(TodoStore::new(path).await?);
        let mut guard = self.store.write().await;
        *guard = Some(store.clone());
        Ok(store)
    }
}

fn notes_with_explicit_sizing(
    notes: Option<&str>,
    t_shirt_size: Option<&str>,
    story_points: Option<i32>,
    estimate_optimistic_minutes: Option<i32>,
    estimate_likely_minutes: Option<i32>,
    estimate_pessimistic_minutes: Option<i32>,
) -> Option<String> {
    let mut chunks = Vec::new();
    if let Some(base) = notes {
        let trimmed = base.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    if let Some(size) = t_shirt_size.map(|v| v.trim()).filter(|v| !v.is_empty()) {
        chunks.push(format!("T-Shirt Size: {}", size.to_ascii_uppercase()));
    }
    if let Some(points) = story_points.filter(|v| *v > 0) {
        chunks.push(format!("Story Points: {points}"));
    }
    if let Some(minutes) = estimate_likely_minutes.filter(|v| *v > 0) {
        chunks.push(format!("Time Estimate: {minutes} minutes"));
    }
    if let Some(minutes) = estimate_optimistic_minutes.filter(|v| *v > 0) {
        chunks.push(format!("Estimate Optimistic Minutes: {minutes}"));
    }
    if let Some(minutes) = estimate_pessimistic_minutes.filter(|v| *v > 0) {
        chunks.push(format!("Estimate Pessimistic Minutes: {minutes}"));
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(" | "))
    }
}

fn parse_dependency_refs(value: Option<&Value>) -> Vec<String> {
    let mut refs = Vec::new();
    if let Some(Value::Array(items)) = value {
        for item in items {
            if let Some(text) = item.as_str() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    refs.push(trimmed.to_ascii_lowercase());
                }
            }
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Manage an ordered todo list (create, list, reorder, complete, delete, clear)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "complete", "reopen", "delete", "clear", "reorder", "create_many"]
                },
                "user_id": { "type": "string" },
                "title": { "type": "string" },
                "notes": { "type": "string" },
                "t_shirt_size": { "type": "string", "enum": ["XS", "S", "M", "L", "XL", "XXL"] },
                "story_points": { "type": "integer" },
                "estimate_optimistic_minutes": { "type": "integer" },
                "estimate_likely_minutes": { "type": "integer" },
                "estimate_pessimistic_minutes": { "type": "integer" },
                "dependency_refs": { "type": "array", "items": { "type": "string" } },
                "items": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "object", "properties": {
                                "title": {"type": "string"},
                                "notes": {"type": "string"},
                                "t_shirt_size": { "type": "string", "enum": ["XS", "S", "M", "L", "XL", "XXL"] },
                                "story_points": { "type": "integer" },
                                "estimate_optimistic_minutes": { "type": "integer" },
                                "estimate_likely_minutes": { "type": "integer" },
                                "estimate_pessimistic_minutes": { "type": "integer" },
                                "dependency_refs": { "type": "array", "items": { "type": "string" } }
                            }}
                        ]
                    }
                },
                "status": { "type": "string", "enum": ["open", "completed", "all"] },
                "limit": { "type": "integer" },
                "id": { "type": "integer" },
                "ordered_ids": { "type": "array", "items": { "type": "integer" } }
            },
            "required": ["action", "user_id"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let path = resolve_todo_db_path(config);
        let mut guard = self
            .sqlite_path
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("Todo tool lock busy".to_string()))?;
        *guard = path;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let action = match action.as_str() {
            "add" | "new" => "create",
            "create_list" | "create_many" | "add_many" | "bulk_create" | "create_items" => {
                "create_many"
            }
            "clear_all" | "delete_all" | "remove_all" | "wipe" | "clean" => "clear",
            other => other,
        };
        let user_id = params
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ButterflyBotError::Runtime("Missing user_id".to_string()))?;

        let store = self.get_store().await?;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        match action {
            "create" => {
                let title = params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing title".to_string()))?;
                let notes = params.get("notes").and_then(|v| v.as_str());
                let notes = notes_with_explicit_sizing(
                    notes,
                    params.get("t_shirt_size").and_then(|v| v.as_str()),
                    params
                        .get("story_points")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32),
                    params
                        .get("estimate_optimistic_minutes")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32),
                    params
                        .get("estimate_likely_minutes")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32),
                    params
                        .get("estimate_pessimistic_minutes")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32),
                );
                let dependency_refs = parse_dependency_refs(params.get("dependency_refs"));
                let item = store
                    .create_item(
                        user_id,
                        title,
                        notes.as_deref(),
                        if dependency_refs.is_empty() {
                            None
                        } else {
                            Some(dependency_refs.as_slice())
                        },
                    )
                    .await?;
                Ok(json!({"status": "ok", "item": item}))
            }
            "create_many" => {
                let items = params
                    .get("items")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing items".to_string()))?;
                if items.is_empty() {
                    return Err(ButterflyBotError::Runtime("items empty".to_string()));
                }
                let mut created = Vec::new();
                for item in items {
                    match item {
                        Value::String(title) => {
                            let created_item =
                                store.create_item(user_id, title, None, None).await?;
                            created.push(created_item);
                        }
                        Value::Object(map) => {
                            let title =
                                map.get("title").and_then(|v| v.as_str()).ok_or_else(|| {
                                    ButterflyBotError::Runtime("Missing item title".to_string())
                                })?;
                            let notes = map.get("notes").and_then(|v| v.as_str());
                            let notes = notes_with_explicit_sizing(
                                notes,
                                map.get("t_shirt_size").and_then(|v| v.as_str()),
                                map.get("story_points")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32),
                                map.get("estimate_optimistic_minutes")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32),
                                map.get("estimate_likely_minutes")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32),
                                map.get("estimate_pessimistic_minutes")
                                    .and_then(|v| v.as_i64())
                                    .map(|v| v as i32),
                            );
                            let dependency_refs = parse_dependency_refs(map.get("dependency_refs"));
                            let created_item = store
                                .create_item(
                                    user_id,
                                    title,
                                    notes.as_deref(),
                                    if dependency_refs.is_empty() {
                                        None
                                    } else {
                                        Some(dependency_refs.as_slice())
                                    },
                                )
                                .await?;
                            created.push(created_item);
                        }
                        _ => {
                            return Err(ButterflyBotError::Runtime(
                                "Invalid item format".to_string(),
                            ))
                        }
                    }
                }
                Ok(json!({"status": "ok", "items": created}))
            }
            "list" => {
                let status = TodoStatus::from_option(params.get("status").and_then(|v| v.as_str()));
                let items = store.list_items(user_id, status, limit).await?;
                Ok(json!({"status": "ok", "items": items}))
            }
            "complete" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let item = store.set_completed(id, true).await?;
                Ok(json!({"status": "ok", "item": item}))
            }
            "reopen" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let item = store.set_completed(id, false).await?;
                Ok(json!({"status": "ok", "item": item}))
            }
            "delete" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let deleted = store.delete_item(id).await?;
                Ok(json!({"status": "ok", "deleted": deleted}))
            }
            "clear" => {
                let status = TodoStatus::from_option(params.get("status").and_then(|v| v.as_str()));
                let deleted = store.clear_items(user_id, status).await?;
                Ok(json!({"status": "ok", "deleted": deleted}))
            }
            "reorder" => {
                let ordered_ids = params
                    .get("ordered_ids")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing ordered_ids".to_string()))?;
                let ids: Vec<i32> = ordered_ids
                    .iter()
                    .filter_map(|value| value.as_i64().map(|v| v as i32))
                    .collect();
                if ids.is_empty() {
                    return Err(ButterflyBotError::Runtime("ordered_ids empty".to_string()));
                }
                store.reorder(user_id, &ids).await?;
                Ok(json!({"status": "ok"}))
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}
