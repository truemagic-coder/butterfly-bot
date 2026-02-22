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

use crate::error::{ButterflyBotError, Result};

mod schema;
use schema::inbox_item_states;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const INBOX_STATES_UP_SQL: &str =
    include_str!("../../migrations/20260221_create_inbox_item_states/up.sql");

type SqliteAsyncConn = SyncConnectionWrapper<SqliteConnection>;
type SqlitePool = Pool<SqliteAsyncConn>;
type SqlitePooledConn<'a> = PooledConnection<'a, SqliteAsyncConn>;

#[derive(Insertable)]
#[diesel(table_name = inbox_item_states)]
struct NewInboxItemState<'a> {
    user_id: &'a str,
    origin_ref: &'a str,
    status: &'a str,
    created_at: i64,
    updated_at: i64,
}

pub struct InboxStateStore {
    pool: SqlitePool,
}

impl InboxStateStore {
    pub async fn new(sqlite_path: impl AsRef<str>) -> Result<Self> {
        let sqlite_path = sqlite_path.as_ref();
        ensure_parent_dir(sqlite_path)?;
        run_migrations(sqlite_path).await?;
        ensure_inbox_states_table(sqlite_path).await?;

        let manager = AsyncDieselConnectionManager::<SqliteAsyncConn>::new(sqlite_path);
        let pool: SqlitePool = Pool::builder()
            .build(manager)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn set_status(&self, user_id: &str, origin_ref: &str, status: &str) -> Result<()> {
        let now = now_ts();
        let mut conn = self.conn().await?;

        let existing = inbox_item_states::table
            .filter(inbox_item_states::user_id.eq(user_id))
            .filter(inbox_item_states::origin_ref.eq(origin_ref))
            .select(inbox_item_states::id)
            .first::<i32>(&mut conn)
            .await
            .optional()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        if existing.is_some() {
            diesel::update(
                inbox_item_states::table
                    .filter(inbox_item_states::user_id.eq(user_id))
                    .filter(inbox_item_states::origin_ref.eq(origin_ref)),
            )
            .set((
                inbox_item_states::status.eq(status),
                inbox_item_states::updated_at.eq(now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            return Ok(());
        }

        let new_row = NewInboxItemState {
            user_id,
            origin_ref,
            status,
            created_at: now,
            updated_at: now,
        };

        diesel::insert_into(inbox_item_states::table)
            .values(&new_row)
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        Ok(())
    }

    pub async fn list_statuses(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<HashMap<String, String>> {
        let mut conn = self.conn().await?;
        let rows: Vec<(String, String)> = inbox_item_states::table
            .filter(inbox_item_states::user_id.eq(user_id))
            .order(inbox_item_states::updated_at.desc())
            .limit(limit as i64)
            .select((inbox_item_states::origin_ref, inbox_item_states::status))
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let mut map = HashMap::with_capacity(rows.len());
        for (origin_ref, status) in rows {
            map.insert(origin_ref, status);
        }
        Ok(map)
    }

    pub async fn clear_statuses(&self, user_id: &str) -> Result<usize> {
        let mut conn = self.conn().await?;
        let deleted =
            diesel::delete(inbox_item_states::table.filter(inbox_item_states::user_id.eq(user_id)))
                .execute(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(deleted)
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

async fn ensure_inbox_states_table(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = crate::db::open_sqlcipher_connection_sync(&database_url)?;

        let check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT 1 FROM inbox_item_states LIMIT 1",
        );
        if let Err(err) = check {
            let message = err.to_string();
            if message.contains("no such table") {
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                diesel::connection::SimpleConnection::batch_execute(&mut conn, INBOX_STATES_UP_SQL)
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

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
