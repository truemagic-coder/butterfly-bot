use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::planning::{default_plan_db_path, resolve_plan_db_path, PlanStore};
use crate::todo::{TodoStatus, TodoStore};

pub struct PlanningTool {
    sqlite_path: RwLock<Option<String>>,
    store: RwLock<Option<std::sync::Arc<PlanStore>>>,
    todo_store: RwLock<Option<std::sync::Arc<TodoStore>>>,
}

impl Default for PlanningTool {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanningTool {
    pub fn new() -> Self {
        Self {
            sqlite_path: RwLock::new(None),
            store: RwLock::new(None),
            todo_store: RwLock::new(None),
        }
    }

    async fn get_store(&self) -> Result<std::sync::Arc<PlanStore>> {
        if let Some(store) = self.store.read().await.as_ref() {
            return Ok(store.clone());
        }
        let path = self
            .sqlite_path
            .read()
            .await
            .clone()
            .unwrap_or_else(default_plan_db_path);
        let store = std::sync::Arc::new(PlanStore::new(path).await?);
        let mut guard = self.store.write().await;
        *guard = Some(store.clone());
        Ok(store)
    }

    async fn get_todo_store(&self) -> Result<std::sync::Arc<TodoStore>> {
        if let Some(store) = self.todo_store.read().await.as_ref() {
            return Ok(store.clone());
        }
        let path = self
            .sqlite_path
            .read()
            .await
            .clone()
            .unwrap_or_else(default_plan_db_path);
        let store = std::sync::Arc::new(TodoStore::new(path).await?);
        let mut guard = self.todo_store.write().await;
        *guard = Some(store.clone());
        Ok(store)
    }

    fn parse_step_title(step: &Value) -> Option<String> {
        if let Some(text) = step.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        for key in ["title", "description", "name", "text", "step"] {
            if let Some(value) = step.get(key).and_then(|v| v.as_str()) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    fn step_notes(step: &Value, plan_id: i32, step_index: usize) -> String {
        let mut notes = Vec::new();
        notes.push(format!("PlanStepRef: plan_step:{plan_id}:{step_index}"));
        let dependency_refs = Self::parse_step_dependency_refs(step, plan_id);
        if !dependency_refs.is_empty() {
            notes.push(format!("Depends On: {}", dependency_refs.join(", ")));
        }

        if let Some(owner) = step.get("owner").and_then(|v| v.as_str()) {
            notes.push(format!("Owner: {owner}"));
        }
        if let Some(priority) = step.get("priority").and_then(|v| v.as_str()) {
            notes.push(format!("Priority: {priority}"));
        }
        if let Some(size) = step
            .get("t_shirt_size")
            .or_else(|| step.get("size"))
            .and_then(|v| v.as_str())
        {
            notes.push(format!("T-Shirt Size: {}", size.to_ascii_uppercase()));
        }
        if let Some(points) = step
            .get("story_points")
            .or_else(|| step.get("points"))
            .and_then(|v| v.as_i64())
        {
            if points > 0 {
                notes.push(format!("Story Points: {points}"));
            }
        }

        if let Some(value) = step.get("due_date").and_then(|v| v.as_str()) {
            notes.push(format!("Due Date: {value}"));
        } else if let Some(value) = step.get("due").and_then(|v| v.as_str()) {
            notes.push(format!("Due Date: {value}"));
        } else if let Some(value) = step.get("due_at") {
            if let Some(raw) = value.as_i64() {
                notes.push(format!("Due At: {raw}"));
            } else if let Some(raw) = value.as_str() {
                notes.push(format!("Due Date: {raw}"));
            }
        }

        if let Some(minutes) = step
            .get("estimate_likely_minutes")
            .or_else(|| step.get("likely_minutes"))
            .and_then(|v| v.as_i64())
        {
            if minutes > 0 {
                notes.push(format!("Time Estimate: {minutes} minutes"));
            }
        } else if let Some(estimate_text) = step
            .get("time_estimate")
            .or_else(|| step.get("estimate"))
            .and_then(|v| v.as_str())
        {
            notes.push(format!("Time Estimate: {estimate_text}"));
        }

        if let Some(minutes) = step
            .get("estimate_optimistic_minutes")
            .and_then(|v| v.as_i64())
        {
            if minutes > 0 {
                notes.push(format!("Estimate Optimistic Minutes: {minutes}"));
            }
        }
        if let Some(minutes) = step
            .get("estimate_pessimistic_minutes")
            .and_then(|v| v.as_i64())
        {
            if minutes > 0 {
                notes.push(format!("Estimate Pessimistic Minutes: {minutes}"));
            }
        }

        notes.join(" | ")
    }

    fn parse_step_dependency_refs(step: &Value, plan_id: i32) -> Vec<String> {
        fn push_ref(out: &mut Vec<String>, plan_id: i32, value: &Value) {
            match value {
                Value::Array(values) => {
                    for entry in values {
                        push_ref(out, plan_id, entry);
                    }
                }
                Value::String(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        return;
                    }
                    let normalized = if trimmed.starts_with("plan_step:")
                        || trimmed.starts_with("todo:")
                        || trimmed.starts_with("task:")
                        || trimmed.starts_with("reminder:")
                    {
                        trimmed.to_ascii_lowercase()
                    } else if let Ok(step_index) = trimmed.parse::<usize>() {
                        format!("plan_step:{plan_id}:{step_index}")
                    } else {
                        trimmed.to_ascii_lowercase()
                    };
                    if !out.iter().any(|existing| existing == &normalized) {
                        out.push(normalized);
                    }
                }
                Value::Number(number) => {
                    if let Some(step_index) = number.as_u64() {
                        let normalized = format!("plan_step:{plan_id}:{step_index}");
                        if !out.iter().any(|existing| existing == &normalized) {
                            out.push(normalized);
                        }
                    }
                }
                Value::Object(map) => {
                    if let Some(origin_ref) = map.get("origin_ref") {
                        push_ref(out, plan_id, origin_ref);
                    } else if let Some(id) = map.get("id") {
                        push_ref(out, plan_id, id);
                    } else if let Some(step_index) = map.get("step_index") {
                        push_ref(out, plan_id, step_index);
                    }
                }
                _ => {}
            }
        }

        let mut refs = Vec::new();
        for key in [
            "dependency_refs",
            "depends_on",
            "dependencies",
            "blocked_by",
            "requires",
        ] {
            if let Some(value) = step.get(key) {
                push_ref(&mut refs, plan_id, value);
            }
        }
        refs
    }

    fn normalize_step_owner(raw: Option<&str>) -> &'static str {
        match raw.unwrap_or("human").trim().to_ascii_lowercase().as_str() {
            "agent" | "ai" | "assistant" => "agent",
            _ => "human",
        }
    }

    fn normalize_step_priority(raw: Option<&str>) -> &'static str {
        match raw.unwrap_or("normal").trim().to_ascii_lowercase().as_str() {
            "urgent" | "p0" => "urgent",
            "high" | "p1" => "high",
            "low" | "p3" => "low",
            _ => "normal",
        }
    }

    fn normalize_steps_input(steps: Option<&Value>) -> Result<Option<Value>> {
        fn collect_dependency_refs_raw(step: &Value) -> Vec<String> {
            fn push_refs_from_text(out: &mut Vec<String>, text: &str) {
                let lower = text.to_ascii_lowercase();
                for marker in [
                    "depends on:",
                    "dependencies:",
                    "blocked by:",
                    "requires:",
                    "dependency refs:",
                    "dependency_refs:",
                ] {
                    if let Some(start) = lower.find(marker) {
                        let raw = &text[start + marker.len()..];
                        let line = raw.split('\n').next().unwrap_or(raw);
                        for token in line.split([',', '|', ';']) {
                            let normalized = token.trim().to_ascii_lowercase();
                            if !normalized.is_empty()
                                && !out.iter().any(|existing| existing == &normalized)
                            {
                                out.push(normalized);
                            }
                        }
                    }
                }
            }

            fn push_ref(out: &mut Vec<String>, value: &Value) {
                match value {
                    Value::Array(values) => {
                        for entry in values {
                            push_ref(out, entry);
                        }
                    }
                    Value::String(text) => {
                        let trimmed = text.trim();
                        if trimmed.is_empty() {
                            return;
                        }
                        if trimmed.contains(',') || trimmed.contains('|') || trimmed.contains(';') {
                            for token in trimmed.split([',', '|', ';']) {
                                push_ref(out, &Value::String(token.to_string()));
                            }
                        } else {
                            let normalized = trimmed.to_ascii_lowercase();
                            if !out.iter().any(|existing| existing == &normalized) {
                                out.push(normalized);
                            }
                        }
                    }
                    Value::Number(number) => {
                        let normalized = number.to_string();
                        if !out.iter().any(|existing| existing == &normalized) {
                            out.push(normalized);
                        }
                    }
                    Value::Object(map) => {
                        if let Some(origin_ref) = map.get("origin_ref") {
                            push_ref(out, origin_ref);
                        } else if let Some(id) = map.get("id") {
                            push_ref(out, id);
                        } else if let Some(step_index) = map
                            .get("step_index")
                            .or_else(|| map.get("index"))
                            .or_else(|| map.get("step"))
                        {
                            push_ref(out, step_index);
                        }
                    }
                    _ => {}
                }
            }

            let mut refs = Vec::new();
            for key in [
                "dependency_refs",
                "dependencyRefs",
                "depends_on",
                "dependsOn",
                "dependencies",
                "blocked_by",
                "blockedBy",
                "requires",
                "prerequisite",
                "prerequisites",
                "after",
            ] {
                if let Some(value) = step.get(key) {
                    push_ref(&mut refs, value);
                }
            }
            for key in ["title", "description", "text", "step", "name"] {
                if let Some(value) = step.get(key).and_then(|v| v.as_str()) {
                    push_refs_from_text(&mut refs, value);
                }
            }
            if let Some(text) = step.as_str() {
                push_refs_from_text(&mut refs, text);
            }
            refs
        }

        let Some(raw_steps) = steps else {
            return Ok(None);
        };
        let Some(items) = raw_steps.as_array() else {
            return Err(ButterflyBotError::Runtime(
                "steps must be an array".to_string(),
            ));
        };

        let mut normalized = Vec::with_capacity(items.len());
        for item in items {
            let mut obj = serde_json::Map::new();
            let title = Self::parse_step_title(item).ok_or_else(|| {
                ButterflyBotError::Runtime(
                    "each step must include title/description/name/text".to_string(),
                )
            })?;
            obj.insert("title".to_string(), Value::String(title));

            if let Some(source_obj) = item.as_object() {
                if let Some(id) = source_obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    obj.insert("id".to_string(), Value::String(id.to_string()));
                }
                if let Some(description) = source_obj
                    .get("description")
                    .or_else(|| source_obj.get("details"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    obj.insert(
                        "description".to_string(),
                        Value::String(description.to_string()),
                    );
                }
                obj.insert(
                    "owner".to_string(),
                    Value::String(
                        Self::normalize_step_owner(
                            source_obj.get("owner").and_then(|v| v.as_str()),
                        )
                        .to_string(),
                    ),
                );
                obj.insert(
                    "priority".to_string(),
                    Value::String(
                        Self::normalize_step_priority(
                            source_obj.get("priority").and_then(|v| v.as_str()),
                        )
                        .to_string(),
                    ),
                );

                if let Some(value) = source_obj
                    .get("due_date")
                    .or_else(|| source_obj.get("due"))
                    .or_else(|| source_obj.get("deadline"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    obj.insert("due_date".to_string(), Value::String(value.to_string()));
                }

                if let Some(value) = source_obj.get("due_at").or_else(|| source_obj.get("dueAt")) {
                    match value {
                        Value::Number(_) | Value::String(_) => {
                            obj.insert("due_at".to_string(), value.clone());
                        }
                        _ => {}
                    }
                }

                if let Some(size) = source_obj
                    .get("t_shirt_size")
                    .or_else(|| source_obj.get("size"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_ascii_uppercase())
                    .filter(|s| matches!(s.as_str(), "XS" | "S" | "M" | "L" | "XL" | "XXL"))
                {
                    obj.insert("t_shirt_size".to_string(), Value::String(size));
                }

                if let Some(points) = source_obj
                    .get("story_points")
                    .or_else(|| source_obj.get("points"))
                    .and_then(|v| v.as_i64())
                    .filter(|v| *v > 0)
                {
                    obj.insert(
                        "story_points".to_string(),
                        Value::Number(serde_json::Number::from(points)),
                    );
                }

                for key in [
                    "estimate_optimistic_minutes",
                    "estimate_likely_minutes",
                    "estimate_pessimistic_minutes",
                ] {
                    if let Some(minutes) = source_obj
                        .get(key)
                        .and_then(|v| v.as_i64())
                        .filter(|v| *v > 0)
                    {
                        obj.insert(
                            key.to_string(),
                            Value::Number(serde_json::Number::from(minutes)),
                        );
                    }
                }
            } else {
                obj.insert("owner".to_string(), Value::String("human".to_string()));
                obj.insert("priority".to_string(), Value::String("normal".to_string()));
            }

            let dependency_refs = collect_dependency_refs_raw(item);
            if !dependency_refs.is_empty() {
                obj.insert(
                    "dependency_refs".to_string(),
                    Value::Array(
                        dependency_refs
                            .into_iter()
                            .map(Value::String)
                            .collect::<Vec<_>>(),
                    ),
                );
            }

            normalized.push(Value::Object(obj));
        }

        Ok(Some(Value::Array(normalized)))
    }

    async fn materialize_steps_as_todos(
        &self,
        user_id: &str,
        plan_id: i32,
        steps: Option<&Value>,
    ) -> Result<usize> {
        let Some(step_values) = steps.and_then(|value| value.as_array()) else {
            return Ok(0);
        };
        if step_values.is_empty() {
            return Ok(0);
        }

        let todo_store = self.get_todo_store().await?;
        let existing = todo_store
            .list_items(user_id, TodoStatus::All, 5000)
            .await
            .unwrap_or_default();
        let existing_refs = existing
            .iter()
            .filter_map(|item| item.notes.as_deref())
            .filter_map(|notes| {
                notes.split('|').map(|part| part.trim()).find_map(|part| {
                    part.strip_prefix("PlanStepRef:")
                        .map(|value| value.trim().to_string())
                })
            })
            .collect::<HashSet<_>>();

        let mut created = 0usize;
        for (index, step) in step_values.iter().enumerate() {
            let Some(title) = Self::parse_step_title(step) else {
                continue;
            };
            let origin_ref = format!("plan_step:{plan_id}:{index}");
            if existing_refs.contains(&origin_ref) {
                continue;
            }

            let notes = Self::step_notes(step, plan_id, index);
            let dependency_refs = Self::parse_step_dependency_refs(step, plan_id);
            todo_store
                .create_item(
                    user_id,
                    &title,
                    Some(&notes),
                    if dependency_refs.is_empty() {
                        None
                    } else {
                        Some(dependency_refs.as_slice())
                    },
                )
                .await?;
            created += 1;
        }

        Ok(created)
    }
}

#[async_trait]
impl Tool for PlanningTool {
    fn name(&self) -> &str {
        "planning"
    }

    fn description(&self) -> &str {
        "Create and manage structured plans with goals and steps."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "get", "update", "delete", "clear"]
                },
                "user_id": { "type": "string" },
                "id": { "type": "integer" },
                "title": { "type": "string" },
                "goal": { "type": "string" },
                "steps": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "description": { "type": "string" },
                                    "owner": { "type": "string", "enum": ["human", "agent"] },
                                    "priority": { "type": "string", "enum": ["low", "normal", "high", "urgent"] },
                                    "due_date": { "type": "string" },
                                    "due_at": { "type": ["integer", "string"] },
                                    "t_shirt_size": { "type": "string", "enum": ["XS", "S", "M", "L", "XL", "XXL"] },
                                    "story_points": { "type": "integer" },
                                    "estimate_optimistic_minutes": { "type": "integer" },
                                    "estimate_likely_minutes": { "type": "integer" },
                                    "estimate_pessimistic_minutes": { "type": "integer" },
                                    "time_estimate": { "type": "string" },
                                    "dependency_refs": { "type": ["array", "string"], "items": { "type": "string" } },
                                    "depends_on": { "type": ["array", "string"], "items": { "type": ["string", "integer"] } },
                                    "dependencies": { "type": ["array", "string"], "items": { "type": ["string", "integer"] } },
                                    "blocked_by": { "type": ["array", "string"], "items": { "type": ["string", "integer"] } },
                                    "requires": { "type": ["array", "string"], "items": { "type": ["string", "integer"] } }
                                }
                            }
                        ]
                    }
                },
                "status": { "type": "string" },
                "limit": { "type": "integer" }
            },
            "required": ["action", "user_id"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let path = resolve_plan_db_path(config);
        let mut guard = self
            .sqlite_path
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("Planning tool lock busy".to_string()))?;
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
            "clear_all" | "delete_all" | "remove_all" | "wipe" | "clean" => "clear",
            other => other,
        };
        let user_id = params
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ButterflyBotError::Runtime("Missing user_id".to_string()))?;

        let store = self.get_store().await?;
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match action {
            "create" => {
                let title = params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing title".to_string()))?;
                let goal = params
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing goal".to_string()))?;
                let normalized_steps = Self::normalize_steps_input(params.get("steps"))?;
                let status = params.get("status").and_then(|v| v.as_str());
                let plan = store
                    .create_plan(user_id, title, goal, normalized_steps.as_ref(), status)
                    .await?;
                let todo_items_created = self
                    .materialize_steps_as_todos(user_id, plan.id, plan.steps.as_ref())
                    .await?;
                Ok(json!({"status": "ok", "plan": plan, "todo_items_created": todo_items_created}))
            }
            "list" => {
                let plans = store.list_plans(user_id, limit).await?;
                Ok(json!({"status": "ok", "plans": plans}))
            }
            "get" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let plan = store.get_plan(id).await?;
                Ok(json!({"status": "ok", "plan": plan}))
            }
            "update" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let title = params.get("title").and_then(|v| v.as_str());
                let goal = params.get("goal").and_then(|v| v.as_str());
                let normalized_steps = Self::normalize_steps_input(params.get("steps"))?;
                let status = params.get("status").and_then(|v| v.as_str());
                let plan = store
                    .update_plan(id, title, goal, normalized_steps.as_ref(), status)
                    .await?;
                let todo_items_created = self
                    .materialize_steps_as_todos(user_id, plan.id, plan.steps.as_ref())
                    .await?;
                Ok(json!({"status": "ok", "plan": plan, "todo_items_created": todo_items_created}))
            }
            "delete" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing id".to_string()))?
                    as i32;
                let deleted = store.delete_plan(id).await?;
                Ok(json!({"status": "ok", "deleted": deleted}))
            }
            "clear" => {
                let deleted = store.clear_plans(user_id).await?;
                Ok(json!({"status": "ok", "deleted": deleted}))
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}
