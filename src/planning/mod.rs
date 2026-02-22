use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_async::pooled_connection::bb8::{Pool, PooledConnection};
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use diesel_async::RunQueryDsl;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use serde::Serialize;
use serde_json::Value;

use crate::error::{ButterflyBotError, Result};

mod schema;
use schema::{plan_step_dependencies, plans};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const PLANS_UP_SQL: &str = include_str!("../../migrations/20260202_create_plans/up.sql");
const PLAN_STEP_DEP_UP_SQL: &str =
    include_str!("../../migrations/20260222_create_plan_step_dependencies/up.sql");

type SqliteAsyncConn = SyncConnectionWrapper<SqliteConnection>;
type SqlitePool = Pool<SqliteAsyncConn>;
type SqlitePooledConn<'a> = PooledConnection<'a, SqliteAsyncConn>;

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub id: i32,
    pub user_id: String,
    pub title: String,
    pub goal: String,
    pub steps: Option<Value>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Queryable)]
struct PlanRow {
    id: i32,
    user_id: String,
    title: String,
    goal: String,
    steps_json: Option<String>,
    status: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = plans)]
struct NewPlan<'a> {
    user_id: &'a str,
    title: &'a str,
    goal: &'a str,
    steps_json: Option<&'a str>,
    status: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable)]
struct PlanStepDependencyRow {
    id: i32,
    plan_id: i32,
    user_id: String,
    step_ref: String,
    depends_on_ref: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = plan_step_dependencies)]
struct NewPlanStepDependency {
    plan_id: i32,
    user_id: String,
    step_ref: String,
    depends_on_ref: String,
    created_at: i64,
    updated_at: i64,
}

pub struct PlanStore {
    pool: SqlitePool,
}

impl PlanStore {
    pub async fn new(sqlite_path: impl AsRef<str>) -> Result<Self> {
        let sqlite_path = sqlite_path.as_ref();
        ensure_parent_dir(sqlite_path)?;
        run_migrations(sqlite_path).await?;
        ensure_plans_table(sqlite_path).await?;

        let manager = AsyncDieselConnectionManager::<SqliteAsyncConn>::new(sqlite_path);
        let pool: SqlitePool = Pool::builder()
            .build(manager)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn create_plan(
        &self,
        user_id: &str,
        title: &str,
        goal: &str,
        steps: Option<&Value>,
        status: Option<&str>,
    ) -> Result<PlanItem> {
        let now = now_ts();
        let steps_json = steps.map(|value| value.to_string());
        let status = status.unwrap_or("draft");
        let new = NewPlan {
            user_id,
            title,
            goal,
            steps_json: steps_json.as_deref(),
            status,
            created_at: now,
            updated_at: now,
        };

        let mut conn = self.conn().await?;
        diesel::insert_into(plans::table)
            .values(&new)
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let row: PlanRow = plans::table
            .filter(plans::user_id.eq(user_id))
            .order(plans::id.desc())
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        sync_plan_step_dependencies(&mut conn, row.id, user_id, steps).await?;
        Ok(map_row(row))
    }

    pub async fn list_plans(&self, user_id: &str, limit: usize) -> Result<Vec<PlanItem>> {
        let mut conn = self.conn().await?;
        let rows: Vec<PlanRow> = plans::table
            .filter(plans::user_id.eq(user_id))
            .order(plans::created_at.desc())
            .limit(limit as i64)
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(rows.into_iter().map(map_row).collect())
    }

    pub async fn get_plan(&self, id: i32) -> Result<PlanItem> {
        let mut conn = self.conn().await?;
        let row: PlanRow = plans::table
            .filter(plans::id.eq(id))
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(map_row(row))
    }

    pub async fn update_plan(
        &self,
        id: i32,
        title: Option<&str>,
        goal: Option<&str>,
        steps: Option<&Value>,
        status: Option<&str>,
    ) -> Result<PlanItem> {
        let now = now_ts();
        let mut conn = self.conn().await?;

        if let Some(title) = title {
            diesel::update(plans::table.filter(plans::id.eq(id)))
                .set((plans::title.eq(title), plans::updated_at.eq(now)))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        if let Some(goal) = goal {
            diesel::update(plans::table.filter(plans::id.eq(id)))
                .set((plans::goal.eq(goal), plans::updated_at.eq(now)))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        if let Some(steps) = steps {
            diesel::update(plans::table.filter(plans::id.eq(id)))
                .set((
                    plans::steps_json.eq(Some(steps.to_string())),
                    plans::updated_at.eq(now),
                ))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            let user: String = plans::table
                .filter(plans::id.eq(id))
                .select(plans::user_id)
                .first(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            sync_plan_step_dependencies(&mut conn, id, &user, Some(steps)).await?;
        }
        if let Some(status) = status {
            diesel::update(plans::table.filter(plans::id.eq(id)))
                .set((plans::status.eq(status), plans::updated_at.eq(now)))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }

        let row: PlanRow = plans::table
            .filter(plans::id.eq(id))
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(map_row(row))
    }

    pub async fn delete_plan(&self, id: i32) -> Result<bool> {
        let mut conn = self.conn().await?;
        let count = diesel::delete(plans::table.filter(plans::id.eq(id)))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(count > 0)
    }

    pub async fn clear_plans(&self, user_id: &str) -> Result<usize> {
        let mut conn = self.conn().await?;
        let plan_ids: Vec<i32> = plans::table
            .filter(plans::user_id.eq(user_id))
            .select(plans::id)
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        if !plan_ids.is_empty() {
            diesel::delete(
                plan_step_dependencies::table
                    .filter(plan_step_dependencies::plan_id.eq_any(&plan_ids)),
            )
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        let deleted = diesel::delete(plans::table.filter(plans::user_id.eq(user_id)))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(deleted)
    }

    pub async fn list_step_dependencies_for_plans(
        &self,
        plan_ids: &[i32],
    ) -> Result<HashMap<String, Vec<String>>> {
        if plan_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn().await?;
        let rows: Vec<PlanStepDependencyRow> = plan_step_dependencies::table
            .filter(plan_step_dependencies::plan_id.eq_any(plan_ids))
            .order((
                plan_step_dependencies::plan_id.asc(),
                plan_step_dependencies::step_ref.asc(),
                plan_step_dependencies::depends_on_ref.asc(),
            ))
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let _ = (
                &row.id,
                &row.plan_id,
                &row.user_id,
                &row.created_at,
                &row.updated_at,
            );
            out.entry(row.step_ref)
                .or_default()
                .push(row.depends_on_ref.to_ascii_lowercase());
        }
        for deps in out.values_mut() {
            deps.sort();
            deps.dedup();
        }
        Ok(out)
    }

    async fn conn(&self) -> Result<SqlitePooledConn<'_>> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        crate::db::apply_sqlcipher_key_async(&mut conn).await?;
        Ok(conn)
    }
}

pub fn resolve_plan_db_path(config: &serde_json::Value) -> Option<String> {
    config
        .get("tools")
        .and_then(|v| v.get("planning"))
        .and_then(|v| v.get("sqlite_path"))
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|path| !path.is_empty())
}

pub fn default_plan_db_path() -> String {
    crate::runtime_paths::default_db_path()
}

fn ensure_parent_dir(path: &str) -> Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    }
    Ok(())
}

async fn run_migrations(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_sqlcipher_connection_sync(&database_url)?;
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok::<_, ButterflyBotError>(())
    })
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
    Ok(())
}

async fn ensure_plans_table(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_sqlcipher_connection_sync(&database_url)?;

        let check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT 1 FROM plans LIMIT 1",
        );
        if let Err(err) = check {
            let message = err.to_string();
            if message.contains("no such table") {
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                diesel::connection::SimpleConnection::batch_execute(&mut conn, PLANS_UP_SQL)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            } else {
                return Err(ButterflyBotError::Runtime(message));
            }
        }

        diesel::connection::SimpleConnection::batch_execute(&mut conn, PLAN_STEP_DEP_UP_SQL)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        Ok::<_, ButterflyBotError>(())
    })
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
    Ok(())
}

fn map_row(row: PlanRow) -> PlanItem {
    PlanItem {
        id: row.id,
        user_id: row.user_id,
        title: row.title,
        goal: row.goal,
        steps: row
            .steps_json
            .and_then(|value| serde_json::from_str(&value).ok()),
        status: row.status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn build_step_alias_map(plan_id: i32, steps: &[Value]) -> HashMap<String, String> {
    let mut alias_map = HashMap::new();
    for (index, step) in steps.iter().enumerate() {
        let origin_ref = format!("plan_step:{plan_id}:{index}");
        alias_map.insert(index.to_string(), origin_ref.clone());
        alias_map.insert(format!("step {index}"), origin_ref.clone());
        alias_map.insert(format!("step {}", index + 1), origin_ref.clone());
        for key in ["id", "ref", "key", "code", "step_id"] {
            if let Some(value) = step.get(key).and_then(|v| v.as_str()) {
                let normalized = value.trim().to_ascii_lowercase();
                if !normalized.is_empty() {
                    alias_map.insert(normalized, origin_ref.clone());
                }
            }
        }

        for key in ["title", "name", "description", "text", "step"] {
            if let Some(value) = step.get(key).and_then(|v| v.as_str()) {
                let normalized = value.trim().to_ascii_lowercase();
                if !normalized.is_empty() {
                    alias_map.insert(normalized.clone(), origin_ref.clone());
                    let compact = normalized
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                                c
                            } else {
                                ' '
                            }
                        })
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !compact.is_empty() {
                        alias_map.insert(compact, origin_ref.clone());
                    }
                }
            }
        }
    }
    alias_map
}

fn parse_step_dependency_refs_with_aliases(
    plan_id: i32,
    step: &Value,
    alias_map: &HashMap<String, String>,
) -> Vec<String> {
    fn resolve_alias(alias_map: &HashMap<String, String>, normalized: &str) -> Option<String> {
        if let Some(mapped) = alias_map.get(normalized) {
            return Some(mapped.clone());
        }
        if normalized.len() < 4 {
            return None;
        }
        alias_map
            .iter()
            .filter(|(key, _)| key.contains(normalized) || normalized.contains(key.as_str()))
            .max_by_key(|(key, _)| key.len())
            .map(|(_, value)| value.clone())
    }

    fn push_ref(
        out: &mut Vec<String>,
        plan_id: i32,
        alias_map: &HashMap<String, String>,
        value: &Value,
    ) {
        match value {
            Value::Array(values) => {
                for entry in values {
                    push_ref(out, plan_id, alias_map, entry);
                }
            }
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return;
                }
                let normalized = trimmed.to_ascii_lowercase();
                if normalized.starts_with("plan_step:")
                    || normalized.starts_with("todo:")
                    || normalized.starts_with("task:")
                    || normalized.starts_with("reminder:")
                    || normalized.starts_with("plan:")
                {
                    out.push(normalized);
                } else if let Some(mapped) = resolve_alias(alias_map, &normalized) {
                    out.push(mapped.clone());
                } else if trimmed.contains(',') || trimmed.contains('|') || trimmed.contains(';') {
                    for token in trimmed.split([',', '|', ';']) {
                        push_ref(out, plan_id, alias_map, &Value::String(token.to_string()));
                    }
                } else if let Ok(step_index) = trimmed.parse::<usize>() {
                    out.push(format!("plan_step:{plan_id}:{step_index}"));
                } else {
                    out.push(normalized);
                }
            }
            Value::Number(number) => {
                if let Some(step_index) = number.as_u64() {
                    out.push(format!("plan_step:{plan_id}:{step_index}"));
                }
            }
            Value::Object(map) => {
                if let Some(origin_ref) = map.get("origin_ref") {
                    push_ref(out, plan_id, alias_map, origin_ref);
                } else if let Some(id) = map.get("id") {
                    push_ref(out, plan_id, alias_map, id);
                } else if let Some(step_index) = map
                    .get("step_index")
                    .or_else(|| map.get("index"))
                    .or_else(|| map.get("step"))
                {
                    push_ref(out, plan_id, alias_map, step_index);
                }
            }
            _ => {}
        }
    }

    fn push_refs_from_text(
        out: &mut Vec<String>,
        plan_id: i32,
        alias_map: &HashMap<String, String>,
        text: &str,
    ) {
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
                    push_ref(out, plan_id, alias_map, &Value::String(token.to_string()));
                }
            }
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
            push_ref(&mut refs, plan_id, alias_map, value);
        }
    }
    for key in ["title", "description", "text", "step"] {
        if let Some(value) = step.get(key).and_then(|v| v.as_str()) {
            push_refs_from_text(&mut refs, plan_id, alias_map, value);
        }
    }
    refs.sort();
    refs.dedup();
    refs
}

async fn sync_plan_step_dependencies(
    conn: &mut SqlitePooledConn<'_>,
    plan_id: i32,
    user_id: &str,
    steps: Option<&Value>,
) -> Result<()> {
    diesel::delete(
        plan_step_dependencies::table.filter(plan_step_dependencies::plan_id.eq(plan_id)),
    )
    .execute(conn)
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    let Some(step_values) = steps.and_then(|value| value.as_array()) else {
        return Ok(());
    };
    if step_values.is_empty() {
        return Ok(());
    }

    let alias_map = build_step_alias_map(plan_id, step_values);
    let now = now_ts();
    let mut rows = Vec::new();

    for (index, step) in step_values.iter().enumerate() {
        let step_ref = format!("plan_step:{plan_id}:{index}");
        let dependency_refs = parse_step_dependency_refs_with_aliases(plan_id, step, &alias_map);
        for dep_ref in dependency_refs {
            rows.push(NewPlanStepDependency {
                plan_id,
                user_id: user_id.to_string(),
                step_ref: step_ref.clone(),
                depends_on_ref: dep_ref,
                created_at: now,
                updated_at: now,
            });
        }
    }

    if rows.is_empty() {
        return Ok(());
    }

    for row in rows {
        diesel::insert_or_ignore_into(plan_step_dependencies::table)
            .values(&row)
            .execute(conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    }

    Ok(())
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
