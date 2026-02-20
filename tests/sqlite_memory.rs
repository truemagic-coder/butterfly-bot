use std::sync::OnceLock;
use tempfile::tempdir;

use butterfly_bot::interfaces::providers::MemoryProvider;
use butterfly_bot::providers::sqlite::{SqliteMemoryProvider, SqliteMemoryProviderConfig};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;

fn setup_security_env() {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let root = ROOT
        .get_or_init(|| {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("butterfly-sqlite-memory-tests-root-{unique}"));
            std::fs::create_dir_all(&path).unwrap();
            path
        })
        .clone();

    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(root));
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "sqlite-memory-test-dek".to_string(),
    ));
    butterfly_bot::vault::set_secret("db_encryption_key", "sqlite-memory-test-sqlcipher-key")
        .expect("set deterministic sqlite memory db key");
}

#[tokio::test]
async fn sqlite_memory_appends_and_reads() {
    setup_security_env();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("mem.db");
    let provider =
        SqliteMemoryProvider::new(SqliteMemoryProviderConfig::new(db_path.to_str().unwrap()))
            .await
            .unwrap();

    provider
        .append_message("u1", "user", "hello")
        .await
        .unwrap();
    provider
        .append_message("u1", "assistant", "world")
        .await
        .unwrap();

    let history = provider.get_history("u1", 10).await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].ends_with("user: hello"));
    assert!(history[1].ends_with("assistant: world"));
}

#[tokio::test]
async fn sqlite_memory_search_uses_fts() {
    setup_security_env();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("mem.db");
    let provider =
        SqliteMemoryProvider::new(SqliteMemoryProviderConfig::new(db_path.to_str().unwrap()))
            .await
            .unwrap();

    provider
        .append_message("u2", "user", "ButterFly Bot memory test")
        .await
        .unwrap();

    let results = provider.search("u2", "memory", 5).await.unwrap();
    assert!(results.iter().any(|item| item.contains("memory")));
}

#[tokio::test]
async fn sqlite_memory_clear_history_repairs_memories_fts_before_delete() {
    setup_security_env();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("mem.db");
    let provider =
        SqliteMemoryProvider::new(SqliteMemoryProviderConfig::new(db_path.to_str().unwrap()))
            .await
            .unwrap();

    let mut conn = SqliteConnection::establish(db_path.to_str().unwrap()).unwrap();
    butterfly_bot::db::apply_sqlcipher_key_sync(&mut conn).unwrap();
    conn.batch_execute(
        "INSERT INTO memories (user_id, summary, tags, salience, created_at)
         VALUES ('u3', 'retain me briefly', NULL, NULL, 1);",
    )
    .unwrap();

    conn.batch_execute("DROP TABLE IF EXISTS memories_fts;")
        .unwrap();

    provider.clear_history("u3").await.unwrap();

    let remaining: i64 =
        diesel::sql_query("SELECT COUNT(*) AS count FROM memories WHERE user_id = ?1")
            .bind::<diesel::sql_types::Text, _>("u3")
            .get_result::<CountRow>(&mut conn)
            .unwrap()
            .count;
    assert_eq!(remaining, 0);
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}
