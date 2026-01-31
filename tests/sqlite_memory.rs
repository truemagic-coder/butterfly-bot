use tempfile::tempdir;

use butterfly_bot::interfaces::providers::MemoryProvider;
use butterfly_bot::providers::sqlite::{SqliteMemoryProvider, SqliteMemoryProviderConfig};

#[tokio::test]
async fn sqlite_memory_appends_and_reads() {
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
    assert_eq!(history[0], "user: hello");
    assert_eq!(history[1], "assistant: world");
}

#[tokio::test]
async fn sqlite_memory_search_uses_fts() {
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
