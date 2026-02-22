use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use diesel::dsl::max;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use diesel_async::pooled_connection::bb8::{Pool, PooledConnection};
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use diesel_async::RunQueryDsl;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::error::{ButterflyBotError, Result};

mod schema;
use schema::todo_items;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const TODO_UP_SQL: &str = include_str!("../../migrations/20260202_create_todos/up.sql");

type SqliteAsyncConn = SyncConnectionWrapper<SqliteConnection>;
type SqlitePool = Pool<SqliteAsyncConn>;
type SqlitePooledConn<'a> = PooledConnection<'a, SqliteAsyncConn>;

#[derive(Debug, Clone, Serialize)]
pub struct TodoItem {
    pub id: i32,
    pub user_id: String,
    pub title: String,
    pub notes: Option<String>,
    pub position: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub t_shirt_size: Option<String>,
    pub story_points: Option<i32>,
    pub estimate_optimistic_minutes: Option<i32>,
    pub estimate_likely_minutes: Option<i32>,
    pub estimate_pessimistic_minutes: Option<i32>,
    pub dependency_refs: Vec<String>,
}

#[derive(Queryable)]
struct TodoRow {
    id: i32,
    user_id: String,
    title: String,
    notes: Option<String>,
    position: i32,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
    t_shirt_size: Option<String>,
    story_points: Option<i32>,
    estimate_optimistic_minutes: Option<i32>,
    estimate_likely_minutes: Option<i32>,
    estimate_pessimistic_minutes: Option<i32>,
    dependency_refs: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = todo_items)]
struct NewTodo<'a> {
    user_id: &'a str,
    title: &'a str,
    notes: Option<&'a str>,
    position: i32,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
    t_shirt_size: Option<&'a str>,
    story_points: Option<i32>,
    estimate_optimistic_minutes: Option<i32>,
    estimate_likely_minutes: Option<i32>,
    estimate_pessimistic_minutes: Option<i32>,
    dependency_refs: Option<&'a str>,
}

struct TodoSizingEstimate {
    t_shirt_size: String,
    story_points: i32,
    optimistic_minutes: i32,
    likely_minutes: i32,
    pessimistic_minutes: i32,
}

#[derive(Clone, Copy)]
pub enum TodoStatus {
    Open,
    Completed,
    All,
}

impl TodoStatus {
    pub fn from_option(value: Option<&str>) -> Self {
        match value {
            Some("completed") => Self::Completed,
            Some("open") => Self::Open,
            _ => Self::All,
        }
    }
}

pub struct TodoStore {
    pool: SqlitePool,
}

impl TodoStore {
    pub async fn new(sqlite_path: impl AsRef<str>) -> Result<Self> {
        let sqlite_path = sqlite_path.as_ref();
        ensure_parent_dir(sqlite_path)?;
        run_migrations(sqlite_path).await?;
        ensure_todo_table(sqlite_path).await?;

        let manager = AsyncDieselConnectionManager::<SqliteAsyncConn>::new(sqlite_path);
        let pool: SqlitePool = Pool::builder()
            .build(manager)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn create_item(
        &self,
        user_id: &str,
        title: &str,
        notes: Option<&str>,
        dependency_refs: Option<&[String]>,
    ) -> Result<TodoItem> {
        let now = now_ts();
        let inferred = infer_todo_sizing(title, notes);
        let mut conn = self.conn().await?;
        let max_pos: Option<i32> = todo_items::table
            .filter(todo_items::user_id.eq(user_id))
            .select(max(todo_items::position))
            .first::<Option<i32>>(&mut conn)
            .await
            .unwrap_or(None);
        let position = max_pos.unwrap_or(0) + 1;
        let dependency_refs_json = dependency_refs
            .map(normalize_dependency_refs)
            .filter(|refs| !refs.is_empty())
            .and_then(|refs| serde_json::to_string(&refs).ok());

        let new = NewTodo {
            user_id,
            title,
            notes,
            position,
            created_at: now,
            updated_at: now,
            completed_at: None,
            t_shirt_size: Some(inferred.t_shirt_size.as_str()),
            story_points: Some(inferred.story_points),
            estimate_optimistic_minutes: Some(inferred.optimistic_minutes),
            estimate_likely_minutes: Some(inferred.likely_minutes),
            estimate_pessimistic_minutes: Some(inferred.pessimistic_minutes),
            dependency_refs: dependency_refs_json.as_deref(),
        };

        diesel::insert_into(todo_items::table)
            .values(&new)
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let row: TodoRow = todo_items::table
            .filter(todo_items::user_id.eq(user_id))
            .order(todo_items::id.desc())
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(map_row(row))
    }

    pub async fn list_items(
        &self,
        user_id: &str,
        status: TodoStatus,
        limit: usize,
    ) -> Result<Vec<TodoItem>> {
        let mut conn = self.conn().await?;
        let mut query = todo_items::table
            .filter(todo_items::user_id.eq(user_id))
            .into_boxed();

        match status {
            TodoStatus::Open => {
                query = query.filter(todo_items::completed_at.is_null());
            }
            TodoStatus::Completed => {
                query = query.filter(todo_items::completed_at.is_not_null());
            }
            TodoStatus::All => {}
        }

        let rows: Vec<TodoRow> = query
            .order(todo_items::position.asc())
            .limit(limit as i64)
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(rows.into_iter().map(map_row).collect())
    }

    pub async fn set_completed(&self, id: i32, completed: bool) -> Result<TodoItem> {
        let now = now_ts();
        let completed_at = if completed { Some(now) } else { None };
        let mut conn = self.conn().await?;
        diesel::update(todo_items::table.filter(todo_items::id.eq(id)))
            .set((
                todo_items::completed_at.eq(completed_at),
                todo_items::updated_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let row: TodoRow = todo_items::table
            .filter(todo_items::id.eq(id))
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(map_row(row))
    }

    pub async fn delete_item(&self, id: i32) -> Result<bool> {
        let mut conn = self.conn().await?;
        let count = diesel::delete(todo_items::table.filter(todo_items::id.eq(id)))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(count > 0)
    }

    pub async fn clear_items(&self, user_id: &str, status: TodoStatus) -> Result<usize> {
        let mut conn = self.conn().await?;
        let deleted = match status {
            TodoStatus::Open => diesel::delete(
                todo_items::table
                    .filter(todo_items::user_id.eq(user_id))
                    .filter(todo_items::completed_at.is_null()),
            )
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?,
            TodoStatus::Completed => diesel::delete(
                todo_items::table
                    .filter(todo_items::user_id.eq(user_id))
                    .filter(todo_items::completed_at.is_not_null()),
            )
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?,
            TodoStatus::All => {
                diesel::delete(todo_items::table.filter(todo_items::user_id.eq(user_id)))
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            }
        };
        Ok(deleted)
    }

    pub async fn reorder(&self, user_id: &str, ordered_ids: &[i32]) -> Result<()> {
        let now = now_ts();
        let mut conn = self.conn().await?;
        for (idx, id) in ordered_ids.iter().enumerate() {
            diesel::update(
                todo_items::table
                    .filter(todo_items::user_id.eq(user_id))
                    .filter(todo_items::id.eq(*id)),
            )
            .set((
                todo_items::position.eq((idx + 1) as i32),
                todo_items::updated_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }
        Ok(())
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

pub fn resolve_todo_db_path(config: &serde_json::Value) -> Option<String> {
    config
        .get("tools")
        .and_then(|v| v.get("todo"))
        .and_then(|v| v.get("sqlite_path"))
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|path| !path.is_empty())
}

pub fn default_todo_db_path() -> String {
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

async fn ensure_todo_table(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_sqlcipher_connection_sync(&database_url)?;

        let check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT 1 FROM todo_items LIMIT 1",
        );
        if let Err(err) = check {
            let message = err.to_string();
            if message.contains("no such table") {
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                diesel::connection::SimpleConnection::batch_execute(&mut conn, TODO_UP_SQL)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            } else {
                return Err(ButterflyBotError::Runtime(message));
            }
        }

        for statement in [
            "ALTER TABLE todo_items ADD COLUMN t_shirt_size TEXT",
            "ALTER TABLE todo_items ADD COLUMN story_points INTEGER",
            "ALTER TABLE todo_items ADD COLUMN estimate_optimistic_minutes INTEGER",
            "ALTER TABLE todo_items ADD COLUMN estimate_likely_minutes INTEGER",
            "ALTER TABLE todo_items ADD COLUMN estimate_pessimistic_minutes INTEGER",
            "ALTER TABLE todo_items ADD COLUMN dependency_refs TEXT",
        ] {
            if let Err(err) =
                diesel::connection::SimpleConnection::batch_execute(&mut conn, statement)
            {
                let message = err.to_string().to_ascii_lowercase();
                if !message.contains("duplicate column name") {
                    return Err(ButterflyBotError::Runtime(err.to_string()));
                }
            }
        }

        Ok::<_, ButterflyBotError>(())
    })
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
    Ok(())
}

fn map_row(row: TodoRow) -> TodoItem {
    let mut dependency_refs = row
        .dependency_refs
        .as_deref()
        .map(parse_dependency_refs_raw)
        .unwrap_or_default();

    if dependency_refs.is_empty() {
        if let Some(notes) = row.notes.as_deref() {
            dependency_refs = parse_dependency_refs_from_notes(notes);
        }
    }

    TodoItem {
        id: row.id,
        user_id: row.user_id,
        title: row.title,
        notes: row.notes,
        position: row.position,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
        t_shirt_size: row.t_shirt_size,
        story_points: row.story_points,
        estimate_optimistic_minutes: row.estimate_optimistic_minutes,
        estimate_likely_minutes: row.estimate_likely_minutes,
        estimate_pessimistic_minutes: row.estimate_pessimistic_minutes,
        dependency_refs,
    }
}

fn normalize_dependency_refs(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.to_ascii_lowercase();
        if out.iter().any(|existing| existing == &normalized) {
            continue;
        }
        out.push(normalized);
    }
    out
}

fn parse_dependency_refs_raw(raw: &str) -> Vec<String> {
    if let Ok(values) = serde_json::from_str::<Vec<String>>(raw) {
        return normalize_dependency_refs(&values);
    }
    if let Ok(single) = serde_json::from_str::<String>(raw) {
        return normalize_dependency_refs(&[single]);
    }

    let values = raw
        .split([',', '|', ';'])
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    normalize_dependency_refs(&values)
}

fn parse_dependency_refs_from_notes(notes: &str) -> Vec<String> {
    static DEPENDS_ON_RE: OnceLock<Regex> = OnceLock::new();
    let re = DEPENDS_ON_RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:depends\s*on|dependencies|blocked\s*by|requires|dependency[_\s-]*refs?)\s*:\s*([^|\n\r]+)",
        )
        .expect("valid dependency extraction regex")
    });

    let mut refs = Vec::new();
    for caps in re.captures_iter(notes) {
        if let Some(raw) = caps.get(1).map(|m| m.as_str()) {
            for token in raw.split([',', '|', ';']) {
                let normalized = token.trim().to_ascii_lowercase();
                if normalized.is_empty() {
                    continue;
                }
                refs.push(normalized);
            }
        }
    }
    normalize_dependency_refs(&refs)
}

fn infer_todo_sizing(title: &str, notes: Option<&str>) -> TodoSizingEstimate {
    let raw_text = format!("{} {}", title, notes.unwrap_or_default());

    if let Some(explicit) = parse_explicit_todo_sizing(&raw_text) {
        return explicit;
    }

    let text = format!(
        "{} {}",
        title.to_ascii_lowercase(),
        notes.unwrap_or_default().to_ascii_lowercase()
    );

    let score = [
        ("refactor", 2),
        ("migration", 3),
        ("security", 3),
        ("integration", 2),
        ("test", 1),
        ("ui", 1),
        ("api", 1),
        ("fix", 1),
        ("urgent", 1),
    ]
    .into_iter()
    .fold(1, |acc, (token, weight)| {
        if text.contains(token) {
            acc + weight
        } else {
            acc
        }
    }) + ((text.len() / 90) as i32).clamp(0, 3);

    let (t_shirt_size, story_points) = match score {
        0..=2 => ("XS", 1),
        3..=4 => ("S", 2),
        5..=6 => ("M", 3),
        7..=8 => ("L", 5),
        _ => ("XL", 8),
    };

    let complexity_multiplier =
        if text.contains("migration") || text.contains("security") || text.contains("incident") {
            1.5
        } else if text.contains("integration") || text.contains("cross-team") {
            1.3
        } else if text.contains("cleanup") || text.contains("typo") {
            0.8
        } else {
            1.0
        };

    let likely = ((story_points as f32) * 75.0 * complexity_multiplier).round() as i32;
    let optimistic = ((likely as f32) * 0.55).round() as i32;
    let pessimistic = ((likely as f32) * 1.85).round() as i32;

    TodoSizingEstimate {
        t_shirt_size: t_shirt_size.to_string(),
        story_points,
        optimistic_minutes: optimistic.max(15),
        likely_minutes: likely.max(30),
        pessimistic_minutes: pessimistic.max(45),
    }
}

fn parse_explicit_todo_sizing(text: &str) -> Option<TodoSizingEstimate> {
    static SIZE_RE: OnceLock<Regex> = OnceLock::new();
    static SP_RE: OnceLock<Regex> = OnceLock::new();
    static EST_RE: OnceLock<Regex> = OnceLock::new();

    let size_re =
        SIZE_RE.get_or_init(|| Regex::new(r"(?i)t-?shirt\s*size\s*:\s*(XS|S|M|L|XL|XXL)").unwrap());
    let sp_re = SP_RE.get_or_init(|| Regex::new(r"(?i)story\s*points?\s*:\s*(\d+)").unwrap());
    let est_re = EST_RE.get_or_init(|| {
        Regex::new(r"(?i)time\s*estimate\s*:\s*(\d+)\s*(weeks?|days?|hours?|hrs?|minutes?|mins?|m)")
            .unwrap()
    });

    let size = size_re
        .captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_ascii_uppercase());
    let points = sp_re
        .captures(text)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .filter(|v| *v > 0);

    let likely_from_estimate = est_re.captures(text).and_then(|caps| {
        let value = caps.get(1)?.as_str().parse::<i32>().ok()?.max(1);
        let unit = caps.get(2)?.as_str().to_ascii_lowercase();
        let minutes = if unit.starts_with("week") {
            value * 5 * 8 * 60
        } else if unit.starts_with("day") {
            value * 8 * 60
        } else if unit.starts_with("hour") || unit.starts_with("hr") {
            value * 60
        } else {
            value
        };
        Some(minutes.max(15))
    });

    if size.is_none() && points.is_none() && likely_from_estimate.is_none() {
        return None;
    }

    let story_points = points.unwrap_or(match size.as_deref() {
        Some("XS") => 1,
        Some("S") => 2,
        Some("M") => 3,
        Some("L") => 5,
        Some("XL") | Some("XXL") => 8,
        _ => 3,
    });

    let t_shirt_size = size.unwrap_or_else(|| {
        match story_points {
            0..=1 => "XS",
            2 => "S",
            3..=4 => "M",
            5..=7 => "L",
            _ => "XL",
        }
        .to_string()
    });

    let likely_minutes = likely_from_estimate
        .unwrap_or_else(|| ((story_points as f32) * 75.0).round() as i32)
        .max(30);
    let optimistic_minutes = ((likely_minutes as f32) * 0.55).round() as i32;
    let pessimistic_minutes = ((likely_minutes as f32) * 1.85).round() as i32;

    Some(TodoSizingEstimate {
        t_shirt_size,
        story_points,
        optimistic_minutes: optimistic_minutes.max(15),
        likely_minutes,
        pessimistic_minutes: pessimistic_minutes.max(45),
    })
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
