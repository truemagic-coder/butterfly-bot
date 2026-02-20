use serde_json::json;
use tokio::sync::Mutex;

use butterfly_bot::error::Result;
use butterfly_bot::interfaces::providers::MemoryProvider;
use butterfly_bot::providers::memory::InMemoryMemoryProvider;

struct DummyMemoryProvider {
    messages: Mutex<Vec<(String, String, String)>>,
}

impl DummyMemoryProvider {
    fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl MemoryProvider for DummyMemoryProvider {
    async fn append_message(&self, user_id: &str, role: &str, content: &str) -> Result<()> {
        self.messages.lock().await.push((
            user_id.to_string(),
            role.to_string(),
            content.to_string(),
        ));
        Ok(())
    }

    async fn get_history(&self, user_id: &str, _limit: usize) -> Result<Vec<String>> {
        let guard = self.messages.lock().await;
        Ok(guard
            .iter()
            .filter(|(u, _, _)| u == user_id)
            .map(|(_, role, content)| format!("{}: {}", role, content))
            .collect())
    }

    async fn clear_history(&self, user_id: &str) -> Result<()> {
        let mut guard = self.messages.lock().await;
        guard.retain(|(u, _, _)| u != user_id);
        Ok(())
    }
}

#[tokio::test]
async fn memory_provider_defaults_and_in_memory() {
    let provider = InMemoryMemoryProvider::new();
    provider.append_message("u1", "user", "hi").await.unwrap();
    provider
        .append_message("u1", "assistant", "hello")
        .await
        .unwrap();

    let history = provider.get_history("u1", 1).await.unwrap();
    assert_eq!(history.len(), 1);

    let all = provider.get_history("u1", 0).await.unwrap();
    assert_eq!(all.len(), 2);

    provider.clear_history("u1").await.unwrap();
    assert!(provider.get_history("u1", 0).await.unwrap().is_empty());

    provider
        .store(
            "u2",
            vec![
                json!({"role":"user","content":"a"}),
                json!({"role":"assistant","content":"b"}),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        provider
            .retrieve("u2")
            .await
            .unwrap()
            .lines()
            .collect::<Vec<_>>()
            .len(),
        2
    );
    let retrieved = provider.retrieve("u2").await.unwrap();
    let mut lines = retrieved.lines();
    assert!(lines.next().unwrap_or_default().ends_with("user: a"));
    assert!(lines.next().unwrap_or_default().ends_with("assistant: b"));
    provider.delete("u2").await.unwrap();

    let dummy = DummyMemoryProvider::new();
    dummy
        .store("u4", vec![json!({"role":"user","content":"x"})])
        .await
        .unwrap();
    assert_eq!(dummy.retrieve("u4").await.unwrap(), "user: x");
    dummy.delete("u4").await.unwrap();
    assert_eq!(
        dummy
            .find("any", json!(null), None, None, None)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(dummy.count_documents("any", json!(null)).unwrap(), 0);
}

#[tokio::test]
async fn memory_provider_collection_find_count_and_search_defaults() {
    let provider = InMemoryMemoryProvider::new();

    provider.insert_document("notes", json!({"kind":"todo","title":"Plan sprint"}));
    provider.insert_document("notes", json!({"kind":"note","title":"Meeting recap"}));
    provider.insert_document("notes", json!({"kind":"todo","title":"Ship release"}));

    let all_docs = provider
        .find("notes", json!(null), None, None, None)
        .expect("find all docs");
    assert_eq!(all_docs.len(), 3);

    let todo_docs = provider
        .find("notes", json!({"kind":"todo"}), None, None, None)
        .expect("find filtered docs");
    assert_eq!(todo_docs.len(), 2);

    let todo_count = provider
        .count_documents("notes", json!({"kind":"todo"}))
        .expect("count filtered docs");
    assert_eq!(todo_count, 2);

    let search_results = provider
        .search("u1", "release", 5)
        .await
        .expect("search should be supported and return empty by default");
    assert!(search_results.is_empty());
}
