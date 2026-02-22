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

use crate::error::{ButterflyBotError, Result};

mod schema;
use schema::reminders;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const REMINDERS_UP_SQL: &str = include_str!("../../migrations/20260130_create_reminders/up.sql");

type SqliteAsyncConn = SyncConnectionWrapper<SqliteConnection>;
type SqlitePool = Pool<SqliteAsyncConn>;
type SqlitePooledConn<'a> = PooledConnection<'a, SqliteAsyncConn>;
const CREATE_DEDUP_DUE_AT_WINDOW_SECONDS: i64 = 2;

#[derive(Debug, Clone, Serialize)]
pub struct ReminderItem {
    pub id: i32,
    pub title: String,
    pub due_at: i64,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub fired_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DueReminder {
    pub user_id: String,
    pub item: ReminderItem,
}

#[derive(Queryable)]
struct ReminderRow {
    id: i32,
    user_id: String,
    title: String,
    due_at: i64,
    created_at: i64,
    completed_at: Option<i64>,
    fired_at: Option<i64>,
}

#[derive(Insertable)]
#[diesel(table_name = reminders)]
struct NewReminder<'a> {
    user_id: &'a str,
    title: &'a str,
    due_at: i64,
    created_at: i64,
    completed_at: Option<i64>,
    fired_at: Option<i64>,
}

pub struct ReminderStore {
    pool: SqlitePool,
}

impl ReminderStore {
    pub async fn new(sqlite_path: impl AsRef<str>) -> Result<Self> {
        let sqlite_path = sqlite_path.as_ref();
        ensure_parent_dir(sqlite_path)?;
        run_migrations(sqlite_path).await?;
        ensure_reminders_table(sqlite_path).await?;

        let manager = AsyncDieselConnectionManager::<SqliteAsyncConn>::new(sqlite_path);
        let pool: SqlitePool = Pool::builder()
            .build(manager)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn create_reminder(
        &self,
        user_id: &str,
        title: &str,
        due_at: i64,
    ) -> Result<ReminderItem> {
        let now = now_ts();
        let mut conn = self.conn().await?;

        let existing = reminders::table
            .filter(reminders::user_id.eq(user_id))
            .filter(reminders::title.eq(title))
            .filter(reminders::completed_at.is_null())
            .filter(reminders::fired_at.is_null())
            .filter(reminders::due_at.ge(due_at - CREATE_DEDUP_DUE_AT_WINDOW_SECONDS))
            .filter(reminders::due_at.le(due_at + CREATE_DEDUP_DUE_AT_WINDOW_SECONDS))
            .order(reminders::id.desc())
            .first::<ReminderRow>(&mut conn)
            .await
            .optional()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        if let Some(row) = existing {
            return Ok(map_row(row));
        }

        let new = NewReminder {
            user_id,
            title,
            due_at,
            created_at: now,
            completed_at: None,
            fired_at: None,
        };

        diesel::insert_into(reminders::table)
            .values(&new)
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let row: ReminderRow = reminders::table
            .filter(reminders::user_id.eq(user_id))
            .order(reminders::id.desc())
            .first(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(map_row(row))
    }

    pub async fn list_reminders(
        &self,
        user_id: &str,
        status: ReminderStatus,
        limit: usize,
    ) -> Result<Vec<ReminderItem>> {
        let mut conn = self.conn().await?;
        let mut query = reminders::table
            .filter(reminders::user_id.eq(user_id))
            .into_boxed();

        match status {
            ReminderStatus::Open => {
                query = query.filter(reminders::completed_at.is_null());
            }
            ReminderStatus::Completed => {
                query = query.filter(reminders::completed_at.is_not_null());
            }
            ReminderStatus::All => {}
        }

        if limit > 0 {
            query = query.limit(limit as i64);
        }

        let rows: Vec<ReminderRow> = query
            .order(reminders::due_at.asc())
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(rows.into_iter().map(map_row).collect())
    }

    pub async fn complete_reminder(&self, user_id: &str, id: i32) -> Result<bool> {
        let now = now_ts();
        let mut conn = self.conn().await?;
        let updated = diesel::update(
            reminders::table
                .filter(reminders::user_id.eq(user_id))
                .filter(reminders::id.eq(id)),
        )
        .set(reminders::completed_at.eq(Some(now)))
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(updated > 0)
    }

    pub async fn delete_reminder(&self, user_id: &str, id: i32) -> Result<bool> {
        let mut conn = self.conn().await?;
        let deleted = diesel::delete(
            reminders::table
                .filter(reminders::user_id.eq(user_id))
                .filter(reminders::id.eq(id)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(deleted > 0)
    }

    pub async fn delete_all(&self, user_id: &str, include_completed: bool) -> Result<usize> {
        let mut conn = self.conn().await?;
        let deleted = if include_completed {
            diesel::delete(reminders::table.filter(reminders::user_id.eq(user_id)))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
        } else {
            diesel::delete(
                reminders::table
                    .filter(reminders::user_id.eq(user_id))
                    .filter(reminders::completed_at.is_null()),
            )
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
        };
        Ok(deleted)
    }

    pub async fn snooze_reminder(&self, user_id: &str, id: i32, due_at: i64) -> Result<bool> {
        let mut conn = self.conn().await?;
        let updated = diesel::update(
            reminders::table
                .filter(reminders::user_id.eq(user_id))
                .filter(reminders::id.eq(id)),
        )
        .set((
            reminders::due_at.eq(due_at),
            reminders::fired_at.eq::<Option<i64>>(None),
        ))
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(updated > 0)
    }

    pub async fn due_reminders(
        &self,
        user_id: &str,
        now: i64,
        limit: usize,
    ) -> Result<Vec<ReminderItem>> {
        let mut conn = self.conn().await?;
        let mut query = reminders::table
            .filter(reminders::user_id.eq(user_id))
            .filter(reminders::completed_at.is_null())
            .filter(reminders::due_at.le(now))
            .filter(reminders::fired_at.is_null())
            .into_boxed();
        if limit > 0 {
            query = query.limit(limit as i64);
        }
        let rows: Vec<ReminderRow> = query
            .order(reminders::due_at.asc())
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        if !rows.is_empty() {
            let ids: Vec<i32> = rows.iter().map(|row| row.id).collect();
            diesel::update(
                reminders::table
                    .filter(reminders::user_id.eq(user_id))
                    .filter(reminders::id.eq_any(&ids)),
            )
            .set((
                reminders::fired_at.eq(Some(now)),
                reminders::completed_at.eq(Some(now)),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }

        Ok(rows.into_iter().map(map_row).collect())
    }

    pub async fn due_reminders_all(&self, now: i64, limit: usize) -> Result<Vec<DueReminder>> {
        let rows = self.peek_due_reminders_all_rows(now, limit).await?;

        if !rows.is_empty() {
            let ids: Vec<i32> = rows.iter().map(|row| row.id).collect();
            let mut conn = self.conn().await?;
            diesel::update(reminders::table.filter(reminders::id.eq_any(&ids)))
                .set((
                    reminders::fired_at.eq(Some(now)),
                    reminders::completed_at.eq(Some(now)),
                ))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }

        Ok(rows.into_iter().map(map_due_row).collect())
    }

    pub async fn peek_due_reminders_all(&self, now: i64, limit: usize) -> Result<Vec<DueReminder>> {
        let rows = self.peek_due_reminders_all_rows(now, limit).await?;
        Ok(rows.into_iter().map(map_due_row).collect())
    }

    pub async fn mark_fired_reminder(&self, user_id: &str, id: i32, now: i64) -> Result<bool> {
        let mut conn = self.conn().await?;
        let updated = diesel::update(
            reminders::table
                .filter(reminders::user_id.eq(user_id))
                .filter(reminders::id.eq(id))
                .filter(reminders::completed_at.is_null())
                .filter(reminders::fired_at.is_null()),
        )
        .set((
            reminders::fired_at.eq(Some(now)),
            reminders::completed_at.eq(Some(now)),
        ))
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(updated > 0)
    }

    pub async fn peek_due_reminders(
        &self,
        user_id: &str,
        now: i64,
        limit: usize,
    ) -> Result<Vec<ReminderItem>> {
        let mut conn = self.conn().await?;
        let mut query = reminders::table
            .filter(reminders::user_id.eq(user_id))
            .filter(reminders::completed_at.is_null())
            .filter(reminders::due_at.le(now))
            .filter(reminders::fired_at.is_null())
            .into_boxed();
        if limit > 0 {
            query = query.limit(limit as i64);
        }
        let rows: Vec<ReminderRow> = query
            .order(reminders::due_at.asc())
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(rows.into_iter().map(map_row).collect())
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

    async fn peek_due_reminders_all_rows(
        &self,
        now: i64,
        limit: usize,
    ) -> Result<Vec<ReminderRow>> {
        let mut conn = self.conn().await?;
        let mut query = reminders::table
            .filter(reminders::completed_at.is_null())
            .filter(reminders::due_at.le(now))
            .filter(reminders::fired_at.is_null())
            .into_boxed();
        if limit > 0 {
            query = query.limit(limit as i64);
        }
        query
            .order(reminders::due_at.asc())
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReminderStatus {
    Open,
    Completed,
    All,
}

impl ReminderStatus {
    pub fn from_option(value: Option<&str>) -> Self {
        value
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(ReminderStatus::Open)
    }
}

impl std::str::FromStr for ReminderStatus {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match value {
            "completed" => ReminderStatus::Completed,
            "all" => ReminderStatus::All,
            _ => ReminderStatus::Open,
        })
    }
}

fn map_row(row: ReminderRow) -> ReminderItem {
    ReminderItem {
        id: row.id,
        title: row.title,
        due_at: row.due_at,
        created_at: row.created_at,
        completed_at: row.completed_at,
        fired_at: row.fired_at,
    }
}

fn map_due_row(row: ReminderRow) -> DueReminder {
    let user_id = row.user_id.clone();
    DueReminder {
        user_id,
        item: map_row(row),
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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

async fn ensure_reminders_table(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_sqlcipher_connection_sync(&database_url)?;

        let check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT 1 FROM reminders LIMIT 1",
        );
        if let Err(err) = check {
            let message = err.to_string();
            if message.contains("no such table") {
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                diesel::connection::SimpleConnection::batch_execute(&mut conn, REMINDERS_UP_SQL)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            } else {
                return Err(ButterflyBotError::Runtime(message));
            }
        }

        Ok::<_, ButterflyBotError>(())
    })
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
    Ok(())
}

pub fn resolve_reminder_db_path(config: &serde_json::Value) -> Option<String> {
    let tool_path = config
        .get("tools")
        .and_then(|v| v.get("reminders"))
        .and_then(|v| v.get("sqlite_path"))
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string());
    if let Some(path) = tool_path {
        if !path.is_empty() {
            return Some(path);
        }
    }
    let memory_path = config
        .get("memory")
        .and_then(|v| v.get("sqlite_path"))
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string());
    if let Some(path) = memory_path {
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

pub fn default_reminder_db_path() -> String {
    crate::runtime_paths::default_db_path()
}

#[cfg(test)]
mod tests {
    use super::{ReminderStatus, ReminderStore};

    #[tokio::test]
    async fn reminder_create_deduplicates_near_identical_open_reminders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("reminders.db");
        let db_path = db_path.to_string_lossy().to_string();
        let store = ReminderStore::new(&db_path).await.expect("store");

        let first = store
            .create_reminder("u1", "Feed the cats", 1_771_147_543)
            .await
            .expect("first create");
        let second = store
            .create_reminder("u1", "Feed the cats", 1_771_147_544)
            .await
            .expect("second create");

        assert_eq!(first.id, second.id);

        let items = store
            .list_reminders("u1", ReminderStatus::Open, 50)
            .await
            .expect("list reminders");
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn reminder_create_allows_distinct_due_times() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("reminders.db");
        let db_path = db_path.to_string_lossy().to_string();
        let store = ReminderStore::new(&db_path).await.expect("store");

        let first = store
            .create_reminder("u1", "Feed the cats", 1_771_147_543)
            .await
            .expect("first create");
        let second = store
            .create_reminder("u1", "Feed the cats", 1_771_147_600)
            .await
            .expect("second create");

        assert_ne!(first.id, second.id);

        let items = store
            .list_reminders("u1", ReminderStatus::Open, 50)
            .await
            .expect("list reminders");
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn due_reminders_are_auto_completed_when_fired() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("reminders.db");
        let db_path = db_path.to_string_lossy().to_string();
        let store = ReminderStore::new(&db_path).await.expect("store");

        let now = 1_771_147_543_i64;
        let created = store
            .create_reminder("u1", "Feed the dogs", now - 5)
            .await
            .expect("create reminder");

        let fired = store
            .due_reminders("u1", now, 10)
            .await
            .expect("due reminders");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].id, created.id);

        let open = store
            .list_reminders("u1", ReminderStatus::Open, 10)
            .await
            .expect("open reminders");
        assert!(open.is_empty());

        let completed = store
            .list_reminders("u1", ReminderStatus::Completed, 10)
            .await
            .expect("completed reminders");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, created.id);
        assert!(completed[0].fired_at.is_some());
        assert!(completed[0].completed_at.is_some());
    }

    #[tokio::test]
    async fn due_reminders_all_returns_items_for_multiple_users_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("reminders.db");
        let db_path = db_path.to_string_lossy().to_string();
        let store = ReminderStore::new(&db_path).await.expect("store");

        let now = 1_771_147_543_i64;
        store
            .create_reminder("u1", "Feed the dogs", now - 5)
            .await
            .expect("create reminder u1");
        store
            .create_reminder("u2", "Feed the cats", now - 7)
            .await
            .expect("create reminder u2");

        let due = store
            .due_reminders_all(now, 10)
            .await
            .expect("due reminders all");
        assert_eq!(due.len(), 2);
        assert!(due.iter().any(|entry| entry.user_id == "u1"));
        assert!(due.iter().any(|entry| entry.user_id == "u2"));

        let second = store
            .due_reminders_all(now + 1, 10)
            .await
            .expect("due reminders all second call");
        assert!(second.is_empty());
    }
}
