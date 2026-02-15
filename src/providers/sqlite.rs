use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::sync::Once;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use deadpool_sqlite::{
    rusqlite::{ffi::sqlite3_auto_extension, params, OptionalExtension},
    Config as DeadpoolSqliteConfig, Pool as DeadpoolSqlitePool, Runtime as DeadpoolRuntime,
};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use diesel::sqlite::SqliteConnection;
use diesel_async::pooled_connection::bb8::{Pool, PooledConnection};
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use diesel_async::RunQueryDsl;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use lru::LruCache;
use serde_json::json;
use sqlite_vec::sqlite3_vec_init;
use time::{macros::format_description, OffsetDateTime};
use tracing::{info, warn};

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::providers::{LlmProvider, MemoryProvider};

mod schema;
use schema::messages;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const MEMORY_UP_SQL: &str = include_str!("../../migrations/20250129_create_memory/up.sql");
const CLEAR_HISTORY_MAX_ATTEMPTS: usize = 6;
const CLEAR_HISTORY_RETRY_BASE_MS: u64 = 100;
const MESSAGE_VECTOR_SCHEMA_VERSION: i64 = 1;

type SqliteAsyncConn = SyncConnectionWrapper<SqliteConnection>;
type SqlitePool = Pool<SqliteAsyncConn>;
type SqlitePooledConn<'a> = PooledConnection<'a, SqliteAsyncConn>;

#[derive(Queryable)]
struct MessageRow {
    role: String,
    content: String,
    timestamp: i64,
}

#[derive(Queryable)]
struct MessageHistoryRow {
    id: i32,
    role: String,
    content: String,
    timestamp: i64,
}

#[derive(QueryableByName)]
struct RowId {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    id: i64,
}

#[derive(QueryableByName)]
struct SearchRow {
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = BigInt)]
    timestamp: i64,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct HistoryResetRow {
    #[diesel(sql_type = BigInt)]
    reset_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = messages)]
struct NewMessage<'a> {
    user_id: &'a str,
    role: &'a str,
    content: &'a str,
    timestamp: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::providers::sqlite::schema::memories)]
struct NewMemory<'a> {
    user_id: &'a str,
    summary: &'a str,
    tags: Option<&'a str>,
    salience: Option<f64>,
    created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::providers::sqlite::schema::entities)]
struct NewEntity<'a> {
    user_id: &'a str,
    name: &'a str,
    entity_type: &'a str,
    canonical_id: Option<&'a str>,
    created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::providers::sqlite::schema::facts)]
struct NewFact<'a> {
    user_id: &'a str,
    subject: &'a str,
    predicate: &'a str,
    object: &'a str,
    confidence: Option<f64>,
    source: Option<&'a str>,
    created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::providers::sqlite::schema::edges)]
struct NewEdge<'a> {
    user_id: &'a str,
    src_node_type: &'a str,
    src_node_id: i32,
    dst_node_type: &'a str,
    dst_node_id: i32,
    edge_type: &'a str,
    weight: Option<f64>,
    created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = crate::providers::sqlite::schema::memory_links)]
struct NewMemoryLink<'a> {
    memory_id: i32,
    node_type: &'a str,
    node_id: i32,
    created_at: i64,
}

pub struct SqliteMemoryProvider {
    sqlite_path: String,
    pool: SqlitePool,
    deadpool: DeadpoolSqlitePool,
    write_gate: Arc<tokio::sync::Mutex<()>>,
    embedder: Option<Arc<dyn LlmProvider>>,
    embedding_model: Option<String>,
    reranker: Option<Arc<dyn LlmProvider>>,
    summarizer: Option<Arc<dyn LlmProvider>>,
    summary_threshold: usize,
    retention_days: Option<u32>,
    context_embed_enabled: bool,
    embedding_cache: Arc<tokio::sync::Mutex<LruCache<String, Vec<f32>>>>,
}

impl Clone for SqliteMemoryProvider {
    fn clone(&self) -> Self {
        Self {
            sqlite_path: self.sqlite_path.clone(),
            pool: self.pool.clone(),
            deadpool: self.deadpool.clone(),
            write_gate: Arc::clone(&self.write_gate),
            embedder: self.embedder.clone(),
            embedding_model: self.embedding_model.clone(),
            reranker: self.reranker.clone(),
            summarizer: self.summarizer.clone(),
            summary_threshold: self.summary_threshold,
            retention_days: self.retention_days,
            context_embed_enabled: self.context_embed_enabled,
            embedding_cache: Arc::clone(&self.embedding_cache),
        }
    }
}

pub struct SqliteMemoryProviderConfig {
    pub sqlite_path: String,
    pub embedder: Option<Arc<dyn LlmProvider>>,
    pub embedding_model: Option<String>,
    pub reranker: Option<Arc<dyn LlmProvider>>,
    pub summarizer: Option<Arc<dyn LlmProvider>>,
    pub context_embed_enabled: bool,
    pub summary_threshold: Option<usize>,
    pub retention_days: Option<u32>,
}

impl SqliteMemoryProviderConfig {
    pub fn new(sqlite_path: impl Into<String>) -> Self {
        Self {
            sqlite_path: sqlite_path.into(),
            embedder: None,
            embedding_model: None,
            reranker: None,
            summarizer: None,
            context_embed_enabled: false,
            summary_threshold: None,
            retention_days: None,
        }
    }
}

impl SqliteMemoryProvider {
    pub async fn new(config: SqliteMemoryProviderConfig) -> Result<Self> {
        register_sqlite_vec_extension();
        ensure_parent_dir(&config.sqlite_path)?;
        run_migrations(&config.sqlite_path).await?;
        ensure_memory_tables(&config.sqlite_path).await?;

        let manager =
            AsyncDieselConnectionManager::<SqliteAsyncConn>::new(config.sqlite_path.as_str());
        let pool: SqlitePool = Pool::builder()
            .build(manager)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let deadpool_cfg = DeadpoolSqliteConfig::new(config.sqlite_path.clone());
        let deadpool = deadpool_cfg
            .create_pool(DeadpoolRuntime::Tokio1)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        Ok(Self {
            sqlite_path: config.sqlite_path,
            pool,
            deadpool,
            write_gate: Arc::new(tokio::sync::Mutex::new(())),
            embedder: config.embedder,
            embedding_model: config.embedding_model,
            reranker: config.reranker,
            summarizer: config.summarizer,
            summary_threshold: config.summary_threshold.unwrap_or(12),
            retention_days: config.retention_days,
            context_embed_enabled: config.context_embed_enabled,
            embedding_cache: Arc::new(tokio::sync::Mutex::new(LruCache::new(
                NonZeroUsize::new(256).unwrap(),
            ))),
        })
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

const TIMESTAMP_FORMAT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]");

fn format_timestamp(ts: i64) -> String {
    OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .and_then(|dt| dt.format(TIMESTAMP_FORMAT).ok())
        .unwrap_or_else(|| ts.to_string())
}

fn register_sqlite_vec_extension() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}

fn encode_f32_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
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
        let mut conn = SqliteConnection::establish(&database_url)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        crate::db::apply_sqlcipher_key_sync(&mut conn)?;
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok::<_, ButterflyBotError>(())
    })
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
    Ok(())
}

async fn ensure_memory_tables(database_url: &str) -> Result<()> {
    let database_url = database_url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut conn = SqliteConnection::establish(&database_url)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        crate::db::apply_sqlcipher_key_sync(&mut conn)?;

        let tables = [
            "messages",
            "memories",
            "entities",
            "events",
            "facts",
            "edges",
            "memory_links",
        ];
        for table in tables {
            let query = format!("SELECT 1 FROM {table} LIMIT 1");
            let check = diesel::connection::SimpleConnection::batch_execute(&mut conn, &query);
            if let Err(err) = check {
                let message = err.to_string();
                if message.contains("no such table") {
                    conn.run_pending_migrations(MIGRATIONS)
                        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                    diesel::connection::SimpleConnection::batch_execute(&mut conn, MEMORY_UP_SQL)
                        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                    break;
                } else {
                    return Err(ButterflyBotError::Runtime(message));
                }
            }
        }

        diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "CREATE TABLE IF NOT EXISTS history_resets (
                user_id TEXT PRIMARY KEY,
                reset_at BIGINT NOT NULL
            );",
        )
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "CREATE TABLE IF NOT EXISTS message_vector_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS message_vectors (
                message_id INTEGER PRIMARY KEY,
                user_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp BIGINT NOT NULL,
                embedding BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_message_vectors_user_ts
                ON message_vectors(user_id, timestamp DESC);",
        )
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let fts_check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT message_id FROM messages_fts LIMIT 1",
        );
        if let Err(err) = fts_check {
            let message = err.to_string();
            if message.contains("no such table")
                || message.contains("no such column")
                || message.contains("SQL logic error")
            {
                repair_messages_fts_sync(&mut conn)?;
            } else {
                return Err(ButterflyBotError::Runtime(message));
            }
        }

        let memories_fts_check = diesel::connection::SimpleConnection::batch_execute(
            &mut conn,
            "SELECT memory_id FROM memories_fts LIMIT 1",
        );
        if let Err(err) = memories_fts_check {
            let message = err.to_string();
            if message.contains("no such table")
                || message.contains("no such column")
                || message.contains("SQL logic error")
            {
                diesel::connection::SimpleConnection::batch_execute(
                    &mut conn,
                    REPAIR_MEMORIES_FTS_SQL,
                )
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

fn repair_messages_fts_sync(conn: &mut SqliteConnection) -> Result<()> {
    diesel::connection::SimpleConnection::batch_execute(conn, REPAIR_MESSAGES_FTS_SQL)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}

const REPAIR_MESSAGES_FTS_SQL: &str = r#"
        DROP TRIGGER IF EXISTS messages_ai;
        DROP TRIGGER IF EXISTS messages_ad;
        DROP TRIGGER IF EXISTS messages_au;
        DROP TABLE IF EXISTS messages_fts;

        CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
            content,
            user_id,
            message_id UNINDEXED
        );

        INSERT INTO messages_fts(rowid, content, user_id, message_id)
        SELECT id, content, user_id, id FROM messages;

        CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, content, user_id, message_id)
            VALUES (new.id, new.content, new.user_id, new.id);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content, user_id, message_id)
            VALUES('delete', old.id, old.content, old.user_id, old.id);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content, user_id, message_id)
            VALUES('delete', old.id, old.content, old.user_id, old.id);
            INSERT INTO messages_fts(rowid, content, user_id, message_id)
            VALUES (new.id, new.content, new.user_id, new.id);
        END;
"#;

const REPAIR_MEMORIES_FTS_SQL: &str = r#"
        DROP TRIGGER IF EXISTS memories_ai;
        DROP TRIGGER IF EXISTS memories_ad;
        DROP TRIGGER IF EXISTS memories_au;
        DROP TABLE IF EXISTS memories_fts;

        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            summary,
            user_id,
            memory_id UNINDEXED
        );

        INSERT INTO memories_fts(rowid, summary, user_id, memory_id)
        SELECT id, summary, user_id, id FROM memories;

        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, summary, user_id, memory_id)
            VALUES (new.id, new.summary, new.user_id, new.id);
        END;

        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, summary, user_id, memory_id)
            VALUES('delete', old.id, old.summary, old.user_id, old.id);
        END;

        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, summary, user_id, memory_id)
            VALUES('delete', old.id, old.summary, old.user_id, old.id);
            INSERT INTO memories_fts(rowid, summary, user_id, memory_id)
            VALUES (new.id, new.summary, new.user_id, new.id);
        END;
"#;

fn is_sqlite_locked_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("database is locked")
        || lower.contains("database table is locked")
        || lower.contains("sql logic error")
        || (lower.contains("sql logic error")
            && (lower.contains("locked")
                || lower.contains("busy")
                || lower.contains("sqlite_busy")))
}

async fn get_history_reset_ts(provider: &SqliteMemoryProvider, user_id: &str) -> Result<i64> {
    let mut conn = provider.conn().await?;
    let row = diesel::sql_query("SELECT reset_at FROM history_resets WHERE user_id = ?1 LIMIT 1")
        .bind::<Text, _>(user_id)
        .get_result::<HistoryResetRow>(&mut conn)
        .await;

    match row {
        Ok(value) => Ok(value.reset_at),
        Err(err) => {
            let message = err.to_string();
            if message.contains("NotFound")
                || message.contains("not found")
                || message.contains("no such table")
            {
                Ok(0)
            } else {
                Err(ButterflyBotError::Runtime(message))
            }
        }
    }
}

#[async_trait]
impl MemoryProvider for SqliteMemoryProvider {
    async fn append_message(&self, user_id: &str, role: &str, content: &str) -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs() as i64;
        let row_id = {
            let _write_guard = self.write_gate.lock().await;
            let key = crate::db::get_sqlcipher_key()?;
            let user_id_owned = user_id.to_string();
            let role_owned = role.to_string();
            let content_owned = content.to_string();

            let conn = self
                .deadpool
                .get()
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

            let inserted_id = conn
                .interact(move |conn| -> std::result::Result<i64, String> {
                    conn.execute_batch("PRAGMA busy_timeout = 5000;")
                        .map_err(|e| format!("append_message step=pragma_busy_timeout failed: {e}"))?;

                    if let Some(key) = key {
                        let escaped_key = key.replace('\'', "''");
                        conn.execute_batch(&format!("PRAGMA key = '{escaped_key}';"))
                            .map_err(|e| format!("append_message step=pragma_key failed: {e}"))?;
                    }

                    deadpool_sqlite::rusqlite::Connection::execute(
                        conn,
                        "INSERT INTO messages (user_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
                        params![&user_id_owned, &role_owned, &content_owned, ts],
                    )
                    .map_err(|e| format!("append_message step=insert_message failed: {e}"))?;

                    let mut stmt = conn
                        .prepare("SELECT last_insert_rowid()")
                        .map_err(|e| format!("append_message step=prepare_last_rowid failed: {e}"))?;
                    let id = stmt
                        .query_row([], |row| row.get::<_, i64>(0))
                        .map_err(|e| format!("append_message step=query_last_rowid failed: {e}"))?;
                    Ok(id)
                })
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
                .map_err(ButterflyBotError::Runtime)?;

            RowId { id: inserted_id }
        };

        if let Some(embedder) = &self.embedder {
            let provider = self.clone();
            let embedder = embedder.clone();
            let embedding_model = self.embedding_model.clone();
            let content = content.to_string();
            let role = role.to_string();
            let user_id = user_id.to_string();
            let row_id = row_id.id;
            tokio::spawn(async move {
                let start = Instant::now();
                let vectors = match embedder
                    .embed(vec![content.clone()], embedding_model.as_deref())
                    .await
                {
                    Ok(v) => v,
                    Err(err) => {
                        info!("Embedding failed: {}", err);
                        return;
                    }
                };
                let elapsed = start.elapsed();
                info!(
                    "Embedding computed in {:?} (role={}, chars={}, model={:?})",
                    elapsed,
                    role,
                    content.len(),
                    embedding_model
                );
                if let Some(vector) = vectors.into_iter().next() {
                    let dim = vector.len();
                    if let Err(err) = provider
                        .store_vector_row(row_id, &user_id, &role, &content, ts, vector)
                        .await
                    {
                        info!("sqlite-vec add error: {}", err);
                        return;
                    }
                    info!("Vector stored in sqlite-vec (dim={}, role={})", dim, role);
                }
            });
        }

        if role == "assistant" {
            let provider = self.clone();
            let user_id = user_id.to_string();
            tokio::spawn(async move {
                let _ = provider.maybe_summarize(&user_id).await;
            });
        }

        if let Some(days) = self.retention_days {
            let provider = self.clone();
            let user_id = user_id.to_string();
            tokio::spawn(async move {
                let _ = provider.apply_retention(&user_id, days).await;
            });
        }
        Ok(())
    }

    async fn get_history(&self, user_id: &str, limit: usize) -> Result<Vec<String>> {
        let reset_ts = get_history_reset_ts(self, user_id).await?;
        let mut conn = self.conn().await?;
        let mut query = messages::table
            .filter(messages::user_id.eq(user_id))
            .filter(messages::role.ne("context"))
            .filter(messages::timestamp.gt(reset_ts))
            .order(messages::timestamp.desc())
            .then_order_by(messages::id.desc())
            .select((
                messages::id,
                messages::role,
                messages::content,
                messages::timestamp,
            ))
            .into_boxed();

        if limit > 0 {
            query = query.limit(limit as i64);
        }

        let mut rows: Vec<MessageHistoryRow> = query
            .load(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        rows.sort_by_key(|row| (row.timestamp, row.id));
        Ok(rows
            .into_iter()
            .map(|row| {
                format!(
                    "[{}] {}: {}",
                    format_timestamp(row.timestamp),
                    row.role,
                    row.content
                )
            })
            .collect())
    }

    async fn clear_history(&self, user_id: &str) -> Result<()> {
        ensure_memory_tables(&self.sqlite_path).await?;
        let reset_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs() as i64;
        let _write_guard = self.write_gate.lock().await;

        for attempt in 1..=CLEAR_HISTORY_MAX_ATTEMPTS {
            let sqlite_result = self.clear_history_with_deadpool(user_id, reset_at).await;

            match sqlite_result {
                Ok(_) => {
                    if let Err(err) = self.delete_vector_rows(user_id).await {
                        warn!(
                            "clear_history sqlite-vec delete failed for user_id={}: {}",
                            user_id, err
                        );
                    }
                    info!(
                        "clear_history completed for user_id={} on attempt={}/{} reset_at={}",
                        user_id, attempt, CLEAR_HISTORY_MAX_ATTEMPTS, reset_at
                    );
                    return Ok(());
                }
                Err(err) => {
                    let message = err.to_string();

                    if message.contains("step=delete_messages")
                        && message.to_ascii_lowercase().contains("sql logic error")
                    {
                        warn!(
                            "clear_history detected messages_fts inconsistency for user_id={} on attempt={}/{}; repairing FTS before retry",
                            user_id,
                            attempt,
                            CLEAR_HISTORY_MAX_ATTEMPTS
                        );
                        if let Err(repair_err) = self.repair_messages_fts().await {
                            warn!(
                                "clear_history messages_fts repair failed for user_id={}: {}",
                                user_id, repair_err
                            );
                        } else if attempt < CLEAR_HISTORY_MAX_ATTEMPTS {
                            continue;
                        }
                    }

                    warn!(
                        "clear_history delete failed for user_id={} attempt={}/{}: {}",
                        user_id, attempt, CLEAR_HISTORY_MAX_ATTEMPTS, message
                    );
                    if is_sqlite_locked_error(&message) && attempt < CLEAR_HISTORY_MAX_ATTEMPTS {
                        let backoff_ms = CLEAR_HISTORY_RETRY_BASE_MS * attempt as u64;
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    return Err(ButterflyBotError::Runtime(message));
                }
            }
        }

        Err(ButterflyBotError::Runtime(
            "clear_history marker retries exhausted".to_string(),
        ))
    }

    async fn search(&self, user_id: &str, query: &str, limit: usize) -> Result<Vec<String>> {
        let mut fts_results = self.search_fts(user_id, query, limit).await?;
        if fts_results.len() >= limit.max(1) {
            return Ok(fts_results.into_iter().take(limit.max(1)).collect());
        }
        let trimmed = query.trim();
        let tokens = trimmed.split_whitespace().count();
        let use_vector = tokens >= 4 && trimmed.len() >= 18;

        let vector_results = if use_vector {
            self.search_vector(user_id, query, limit).await?
        } else {
            Vec::new()
        };

        let mut merged = Vec::new();
        for item in fts_results.drain(..).chain(vector_results.into_iter()) {
            if !merged.contains(&item) {
                merged.push(item);
            }
        }

        if let Some(reranker) = &self.reranker {
            if merged.len() > limit.max(1) * 2 {
                let reranked = self
                    .rerank_with_model(reranker, query, &merged, limit)
                    .await?;
                return Ok(reranked);
            }
        }

        Ok(merged.into_iter().take(limit.max(1)).collect())
    }
}

impl SqliteMemoryProvider {
    async fn repair_messages_fts(&self) -> Result<()> {
        let database_url = self.sqlite_path.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = SqliteConnection::establish(&database_url)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            crate::db::apply_sqlcipher_key_sync(&mut conn)?;
            repair_messages_fts_sync(&mut conn)
        })
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))??;
        Ok(())
    }

    async fn clear_history_with_deadpool(&self, user_id: &str, reset_at: i64) -> Result<()> {
        let key = crate::db::get_sqlcipher_key()?;
        let user_id = user_id.to_string();

        let conn = self
            .deadpool
            .get()
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let op_result = conn
            .interact(move |conn| -> std::result::Result<(), String> {
                conn.execute_batch("PRAGMA busy_timeout = 5000;")
                    .map_err(|e| format!("clear_history step=pragma_busy_timeout failed: {e}"))?;

                if let Some(key) = key {
                    let escaped_key = key.replace('\'', "''");
                    conn.execute_batch(&format!("PRAGMA key = '{escaped_key}';"))
                        .map_err(|e| format!("clear_history step=pragma_key failed: {e}"))?;
                }

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM memory_links
                     WHERE memory_id IN (
                        SELECT id FROM memories WHERE user_id = ?1
                     )",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_memory_links failed: {e}"))?;

                conn.execute_batch(
                    "DROP TRIGGER IF EXISTS messages_ai;\n\
                     DROP TRIGGER IF EXISTS messages_ad;\n\
                     DROP TRIGGER IF EXISTS messages_au;\n\
                     DROP TABLE IF EXISTS messages_fts;",
                )
                .map_err(|e| format!("clear_history step=drop_messages_fts failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM messages WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_messages failed: {e}"))?;

                conn.execute_batch(REPAIR_MESSAGES_FTS_SQL)
                    .map_err(|e| format!("clear_history step=repair_messages_fts failed: {e}"))?;

                conn.execute_batch(
                    "DROP TRIGGER IF EXISTS memories_ai;\n\
                     DROP TRIGGER IF EXISTS memories_ad;\n\
                     DROP TRIGGER IF EXISTS memories_au;\n\
                     DROP TABLE IF EXISTS memories_fts;",
                )
                .map_err(|e| format!("clear_history step=drop_memories_fts failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM memories WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_memories failed: {e}"))?;

                conn.execute_batch(REPAIR_MEMORIES_FTS_SQL)
                    .map_err(|e| format!("clear_history step=repair_memories_fts failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM entities WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_entities failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM events WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_events failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM facts WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_facts failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM edges WHERE user_id = ?1",
                    params![&user_id],
                )
                .map_err(|e| format!("clear_history step=delete_edges failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "INSERT OR REPLACE INTO history_resets (user_id, reset_at) VALUES (?1, ?2)",
                    params![&user_id, reset_at],
                )
                .map_err(|e| format!("clear_history step=upsert_history_reset failed: {e}"))?;

                Ok(())
            })
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        op_result.map_err(ButterflyBotError::Runtime)
    }

    async fn store_vector_row(
        &self,
        message_id: i64,
        user_id: &str,
        role: &str,
        content: &str,
        timestamp: i64,
        vector: Vec<f32>,
    ) -> Result<()> {
        let key = crate::db::get_sqlcipher_key()?;
        let user_id = user_id.to_string();
        let role = role.to_string();
        let content = content.to_string();
        let vector_dim = vector.len() as i64;
        let vector_blob = encode_f32_blob(&vector);

        let conn = self
            .deadpool
            .get()
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let op_result = conn
            .interact(move |conn| -> std::result::Result<(), String> {
                conn.execute_batch("PRAGMA busy_timeout = 5000;")
                    .map_err(|e| format!("store_vector step=pragma_busy_timeout failed: {e}"))?;

                if let Some(key) = key {
                    let escaped_key = key.replace('\'', "''");
                    conn.execute_batch(&format!("PRAGMA key = '{escaped_key}';"))
                        .map_err(|e| format!("store_vector step=pragma_key failed: {e}"))?;
                }

                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS message_vector_meta (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS message_vectors (
                        message_id INTEGER PRIMARY KEY,
                        user_id TEXT NOT NULL,
                        role TEXT NOT NULL,
                        content TEXT NOT NULL,
                        timestamp BIGINT NOT NULL,
                        embedding BLOB NOT NULL
                    );",
                )
                .map_err(|e| format!("store_vector step=ensure_tables failed: {e}"))?;

                let existing_dim = conn
                    .query_row(
                        "SELECT value FROM message_vector_meta WHERE key = 'embedding_dim'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(|e| format!("store_vector step=read_dim failed: {e}"))?;

                if let Some(value) = existing_dim {
                    let parsed = value
                        .parse::<i64>()
                        .map_err(|e| format!("store_vector step=parse_dim failed: {e}"))?;
                    if parsed != vector_dim {
                        return Err(format!(
                            "store_vector step=dimension_mismatch failed: expected {}, got {}",
                            parsed, vector_dim
                        ));
                    }
                } else {
                    deadpool_sqlite::rusqlite::Connection::execute(
                        conn,
                        "INSERT INTO message_vector_meta(key, value) VALUES ('embedding_dim', ?1)",
                        params![vector_dim.to_string()],
                    )
                    .map_err(|e| format!("store_vector step=write_dim failed: {e}"))?;
                }

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "INSERT OR REPLACE INTO message_vector_meta(key, value)
                     VALUES ('schema_version', ?1)",
                    params![MESSAGE_VECTOR_SCHEMA_VERSION.to_string()],
                )
                .map_err(|e| format!("store_vector step=write_schema_version failed: {e}"))?;

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "INSERT OR REPLACE INTO message_vectors
                     (message_id, user_id, role, content, timestamp, embedding)
                     VALUES (?1, ?2, ?3, ?4, ?5, vec_f32(?6))",
                    params![
                        message_id,
                        user_id,
                        role,
                        content,
                        timestamp,
                        vector_blob.as_slice()
                    ],
                )
                .map_err(|e| format!("store_vector step=insert failed: {e}"))?;

                Ok(())
            })
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        op_result.map_err(ButterflyBotError::Runtime)
    }

    async fn delete_vector_rows(&self, user_id: &str) -> Result<()> {
        let key = crate::db::get_sqlcipher_key()?;
        let user_id = user_id.to_string();
        let conn = self
            .deadpool
            .get()
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let op_result = conn
            .interact(move |conn| -> std::result::Result<(), String> {
                conn.execute_batch("PRAGMA busy_timeout = 5000;")
                    .map_err(|e| format!("delete_vector step=pragma_busy_timeout failed: {e}"))?;

                if let Some(key) = key {
                    let escaped_key = key.replace('\'', "''");
                    conn.execute_batch(&format!("PRAGMA key = '{escaped_key}';"))
                        .map_err(|e| format!("delete_vector step=pragma_key failed: {e}"))?;
                }

                deadpool_sqlite::rusqlite::Connection::execute(
                    conn,
                    "DELETE FROM message_vectors WHERE user_id = ?1",
                    params![user_id],
                )
                .map_err(|e| format!("delete_vector step=delete failed: {e}"))?;

                Ok(())
            })
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        op_result.map_err(ButterflyBotError::Runtime)
    }

    async fn search_vector_rows(
        &self,
        user_id: &str,
        reset_ts: i64,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<String>> {
        let key = crate::db::get_sqlcipher_key()?;
        let user_id = user_id.to_string();
        let query_blob = encode_f32_blob(vector);

        let conn = self
            .deadpool
            .get()
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let rows = conn
            .interact(
                move |conn| -> std::result::Result<Vec<(String, i64)>, String> {
                    conn.execute_batch("PRAGMA busy_timeout = 5000;")
                        .map_err(|e| {
                            format!("search_vector step=pragma_busy_timeout failed: {e}")
                        })?;

                    if let Some(key) = key {
                        let escaped_key = key.replace('\'', "''");
                        conn.execute_batch(&format!("PRAGMA key = '{escaped_key}';"))
                            .map_err(|e| format!("search_vector step=pragma_key failed: {e}"))?;
                    }

                    let mut stmt = match conn.prepare(
                        "SELECT content, timestamp
                     FROM message_vectors
                     WHERE user_id = ?1
                       AND timestamp > ?2
                     ORDER BY vec_distance_cosine(embedding, vec_f32(?3)) ASC
                     LIMIT ?4",
                    ) {
                        Ok(stmt) => stmt,
                        Err(err) => {
                            let message = err.to_string();
                            if message.contains("no such table") {
                                return Ok(Vec::new());
                            }
                            return Err(format!("search_vector step=prepare failed: {message}"));
                        }
                    };

                    let mapped = stmt
                        .query_map(
                            params![user_id, reset_ts, query_blob.as_slice(), limit as i64],
                            |row| {
                                let content: String = row.get(0)?;
                                let timestamp: i64 = row.get(1)?;
                                Ok((content, timestamp))
                            },
                        )
                        .map_err(|e| format!("search_vector step=query_map failed: {e}"))?;

                    let mut out = Vec::new();
                    for item in mapped {
                        let pair =
                            item.map_err(|e| format!("search_vector step=row failed: {e}"))?;
                        out.push(pair);
                    }
                    Ok(out)
                },
            )
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .map_err(ButterflyBotError::Runtime)?;

        Ok(rows
            .into_iter()
            .map(|(content, timestamp)| format!("[{}] {}", format_timestamp(timestamp), content))
            .collect())
    }

    fn sanitize_fts_query(query: &str) -> Option<String> {
        let mut sanitized = String::with_capacity(query.len());
        for ch in query.chars() {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                sanitized.push(ch);
            } else {
                sanitized.push(' ');
            }
        }
        let trimmed = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            None
        } else {
            Some(format!("\"{}\"", trimmed.replace('"', "")))
        }
    }

    async fn search_fts(&self, user_id: &str, query: &str, limit: usize) -> Result<Vec<String>> {
        let Some(query) = Self::sanitize_fts_query(query) else {
            return Ok(Vec::new());
        };
        let reset_ts = get_history_reset_ts(self, user_id).await?;
        let mut conn = self.conn().await?;
        let rows: Vec<SearchRow> = diesel::sql_query(
            "SELECT mem.summary as content, mem.created_at as timestamp\n             FROM memories_fts f\n             JOIN memories mem ON mem.id = f.memory_id\n             WHERE f.user_id = ?1 AND f.summary MATCH ?2 AND mem.created_at > ?3\n             UNION ALL\n             SELECT m.content as content, m.timestamp as timestamp\n             FROM messages_fts f\n             JOIN messages m ON m.id = f.message_id\n             WHERE f.user_id = ?1 AND f.content MATCH ?2 AND m.role IN ('user','context') AND m.timestamp > ?3\n             ORDER BY timestamp DESC\n             LIMIT ?4",
        )
        .bind::<Text, _>(user_id)
        .bind::<Text, _>(query)
        .bind::<BigInt, _>(reset_ts)
        .bind::<BigInt, _>(limit.max(1) as i64)
        .load(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| format!("[{}] {}", format_timestamp(row.timestamp), row.content))
            .collect())
    }

    async fn search_vector(&self, user_id: &str, query: &str, limit: usize) -> Result<Vec<String>> {
        let reset_ts = get_history_reset_ts(self, user_id).await?;
        let Some(embedder) = &self.embedder else {
            return Ok(Vec::new());
        };

        let model_key = self.embedding_model.as_deref().unwrap_or("default");
        let cache_key = format!("{model_key}:{query}");
        let cached = {
            let mut cache = self.embedding_cache.lock().await;
            cache.get(&cache_key).cloned()
        };
        let vector = if let Some(vector) = cached {
            vector
        } else {
            let vectors = embedder
                .embed(vec![query.to_string()], self.embedding_model.as_deref())
                .await?;
            let Some(vector) = vectors.into_iter().next() else {
                return Ok(Vec::new());
            };
            let mut cache = self.embedding_cache.lock().await;
            cache.put(cache_key, vector.clone());
            vector
        };

        self.search_vector_rows(user_id, reset_ts, &vector, limit.max(1))
            .await
    }

    async fn rerank_with_model(
        &self,
        reranker: &Arc<dyn LlmProvider>,
        query: &str,
        candidates: &[String],
        limit: usize,
    ) -> Result<Vec<String>> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let mut prompt = format!("Query: {query}\n\nCandidates:\n");
        for (idx, item) in candidates.iter().enumerate() {
            prompt.push_str(&format!("{idx}: {item}\n"));
        }
        prompt.push_str("\nReturn JSON {order:[...]} with the best indices in descending relevance. Use at most the requested limit.");

        let schema = json!({
            "type": "object",
            "properties": {
                "order": {"type": "array", "items": {"type": "integer"}}
            },
            "required": ["order"]
        });

        let system = "You are a reranking model. Return the best indices only.";
        let output = reranker
            .parse_structured_output(&prompt, system, schema, None)
            .await
            .unwrap_or_else(|_| json!({"order": []}));

        let order = output
            .get("order")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut ranked = Vec::new();
        for idx in order.into_iter().filter_map(|v| v.as_u64()) {
            let idx = idx as usize;
            if let Some(item) = candidates.get(idx) {
                if !ranked.contains(item) {
                    ranked.push(item.clone());
                }
            }
            if ranked.len() >= limit.max(1) {
                break;
            }
        }

        if ranked.is_empty() {
            Ok(candidates.iter().take(limit.max(1)).cloned().collect())
        } else {
            Ok(ranked)
        }
    }

    pub async fn summarize_now(&self, user_id: &str) -> Result<()> {
        self.summarize_with_threshold(user_id, 1).await
    }

    async fn maybe_summarize(&self, user_id: &str) -> Result<()> {
        self.summarize_with_threshold(user_id, self.summary_threshold)
            .await
    }

    async fn summarize_with_threshold(&self, user_id: &str, threshold: usize) -> Result<()> {
        let Some(summarizer) = &self.summarizer else {
            return Ok(());
        };
        let reset_ts = get_history_reset_ts(self, user_id).await?;
        let rows: Vec<MessageRow> = {
            let mut conn = self.conn().await?;
            let count: CountRow = diesel::sql_query(
                "SELECT COUNT(*) as count FROM messages WHERE user_id = ?1 AND timestamp > ?2",
            )
            .bind::<Text, _>(user_id)
            .bind::<BigInt, _>(reset_ts)
            .get_result(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            if count.count < threshold as i64 {
                return Ok(());
            }

            messages::table
                .filter(messages::user_id.eq(user_id))
                .filter(messages::timestamp.gt(reset_ts))
                .order(messages::timestamp.desc())
                .limit(threshold as i64)
                .select((messages::role, messages::content, messages::timestamp))
                .load(&mut conn)
                .await
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
        };

        let mut rows = rows;
        rows.sort_by_key(|row| row.timestamp);
        let transcript = rows
            .into_iter()
            .map(|row| {
                format!(
                    "[{}] {}: {}",
                    format_timestamp(row.timestamp),
                    row.role,
                    row.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let schema = json!({
            "type": "object",
            "properties": {
                "summary": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "entities": {"type": "array", "items": {"type": "object", "properties": {
                    "name": {"type": "string"},
                    "type": {"type": "string"}
                }, "required": ["name", "type"]}},
                "facts": {"type": "array", "items": {"type": "object", "properties": {
                    "subject": {"type": "string"},
                    "predicate": {"type": "string"},
                    "object": {"type": "string"},
                    "confidence": {"type": "number"}
                }, "required": ["subject", "predicate", "object"]}}
            },
            "required": ["summary"]
        });

        let system = "You are a memory summarizer. Return JSON only.";
        let prompt =
            format!("Summarize the following conversation into a concise memory.\n\n{transcript}");
        let output = summarizer
            .parse_structured_output(&prompt, system, schema, None)
            .await
            .unwrap_or_else(|_| json!({"summary": transcript}));

        let summary = output
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if summary.trim().is_empty() {
            return Ok(());
        }
        let tags = output.get("tags").and_then(|v| v.as_array()).map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        });

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs() as i64;

        let new_memory = NewMemory {
            user_id,
            summary: &summary,
            tags: tags.as_deref(),
            salience: None,
            created_at: now,
        };
        let _write_guard = self.write_gate.lock().await;
        let mut conn = self.conn().await?;
        diesel::insert_into(crate::providers::sqlite::schema::memories::table)
            .values(&new_memory)
            .execute(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let memory_id: RowId = diesel::sql_query("SELECT last_insert_rowid() as id")
            .get_result(&mut conn)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        if let Some(entities) = output.get("entities").and_then(|v| v.as_array()) {
            for entity in entities {
                let Some(name) = entity.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let entity_type = entity
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let new_entity = NewEntity {
                    user_id,
                    name,
                    entity_type,
                    canonical_id: None,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::entities::table)
                    .values(&new_entity)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                let entity_id: RowId = diesel::sql_query("SELECT last_insert_rowid() as id")
                    .get_result(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

                let link = NewMemoryLink {
                    memory_id: memory_id.id as i32,
                    node_type: "entity",
                    node_id: entity_id.id as i32,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::memory_links::table)
                    .values(&link)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

                let edge = NewEdge {
                    user_id,
                    src_node_type: "memory",
                    src_node_id: memory_id.id as i32,
                    dst_node_type: "entity",
                    dst_node_id: entity_id.id as i32,
                    edge_type: "MENTIONED_IN",
                    weight: None,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::edges::table)
                    .values(&edge)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            }
        }

        if let Some(facts) = output.get("facts").and_then(|v| v.as_array()) {
            for fact in facts {
                let (Some(subject), Some(predicate), Some(object)) = (
                    fact.get("subject").and_then(|v| v.as_str()),
                    fact.get("predicate").and_then(|v| v.as_str()),
                    fact.get("object").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                let confidence = fact.get("confidence").and_then(|v| v.as_f64());
                let new_fact = NewFact {
                    user_id,
                    subject,
                    predicate,
                    object,
                    confidence,
                    source: None,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::facts::table)
                    .values(&new_fact)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
                let fact_id: RowId = diesel::sql_query("SELECT last_insert_rowid() as id")
                    .get_result(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

                let link = NewMemoryLink {
                    memory_id: memory_id.id as i32,
                    node_type: "fact",
                    node_id: fact_id.id as i32,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::memory_links::table)
                    .values(&link)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

                let edge = NewEdge {
                    user_id,
                    src_node_type: "memory",
                    src_node_id: memory_id.id as i32,
                    dst_node_type: "fact",
                    dst_node_id: fact_id.id as i32,
                    edge_type: "CONTAINS",
                    weight: None,
                    created_at: now,
                };
                diesel::insert_into(crate::providers::sqlite::schema::edges::table)
                    .values(&edge)
                    .execute(&mut conn)
                    .await
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            }
        }

        Ok(())
    }

    async fn apply_retention(&self, user_id: &str, days: u32) -> Result<()> {
        let cutoff = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs() as i64
            - (days as i64 * 24 * 60 * 60);

        let _write_guard = self.write_gate.lock().await;
        let mut conn = self.conn().await?;
        diesel::delete(
            messages::table.filter(
                messages::user_id
                    .eq(user_id)
                    .and(messages::timestamp.lt(cutoff)),
            ),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        diesel::delete(
            crate::providers::sqlite::schema::memories::table.filter(
                crate::providers::sqlite::schema::memories::user_id
                    .eq(user_id)
                    .and(crate::providers::sqlite::schema::memories::created_at.lt(cutoff)),
            ),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(())
    }
}
