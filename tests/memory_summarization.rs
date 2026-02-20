use std::sync::Arc;
use std::sync::OnceLock;

use async_trait::async_trait;
use serde_json::json;
use tempfile::tempdir;

use butterfly_bot::error::Result;
use butterfly_bot::interfaces::providers::{
    ChatEvent, ImageInput, LlmProvider, LlmResponse, MemoryProvider,
};
use butterfly_bot::providers::sqlite::{SqliteMemoryProvider, SqliteMemoryProviderConfig};

fn setup_security_env() {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let root = ROOT
        .get_or_init(|| {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "butterfly-memory-summarization-tests-root-{unique}"
            ));
            std::fs::create_dir_all(&path).unwrap();
            path
        })
        .clone();

    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(root));
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "memory-summarization-test-dek".to_string(),
    ));
    butterfly_bot::vault::set_secret(
        "db_encryption_key",
        "memory-summarization-test-sqlcipher-key",
    )
    .expect("set deterministic memory summarization db key");
}

struct SummarizerMock;

#[async_trait]
impl LlmProvider for SummarizerMock {
    async fn generate_text(
        &self,
        _prompt: &str,
        _system_prompt: &str,
        _tools: Option<Vec<serde_json::Value>>,
    ) -> Result<String> {
        Ok("ok".to_string())
    }

    async fn generate_with_tools(
        &self,
        _prompt: &str,
        _system_prompt: &str,
        _tools: Vec<serde_json::Value>,
    ) -> Result<LlmResponse> {
        Ok(LlmResponse {
            text: "ok".to_string(),
            tool_calls: Vec::new(),
        })
    }

    fn chat_stream(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Option<Vec<serde_json::Value>>,
    ) -> futures::stream::BoxStream<'static, Result<ChatEvent>> {
        use async_stream::try_stream;
        Box::pin(try_stream! {
            yield ChatEvent {
                event_type: "content".to_string(),
                delta: Some("ok".to_string()),
                name: None,
                arguments_delta: None,
                finish_reason: None,
                error: None,
            };
        })
    }

    async fn parse_structured_output(
        &self,
        _prompt: &str,
        _system_prompt: &str,
        _json_schema: serde_json::Value,
        _tools: Option<Vec<serde_json::Value>>,
    ) -> Result<serde_json::Value> {
        Ok(json!({
            "summary": "user likes ButterFly Bot",
            "tags": ["butterfly", "preference"],
            "entities": [{"name": "ButterFly Bot", "type": "project"}],
            "facts": [{"subject": "user", "predicate": "likes", "object": "ButterFly Bot", "confidence": 0.9}]
        }))
    }

    async fn tts(&self, _text: &str, _voice: &str, _response_format: &str) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn transcribe_audio(&self, _audio_bytes: Vec<u8>, _input_format: &str) -> Result<String> {
        Ok("".to_string())
    }

    async fn generate_text_with_images(
        &self,
        _prompt: &str,
        _images: Vec<ImageInput>,
        _system_prompt: &str,
        _detail: &str,
        _tools: Option<Vec<serde_json::Value>>,
    ) -> Result<String> {
        Ok("".to_string())
    }

    async fn embed(&self, _inputs: Vec<String>, _model: Option<&str>) -> Result<Vec<Vec<f32>>> {
        Ok(vec![vec![0.0, 1.0]])
    }
}

#[tokio::test]
async fn summarization_inserts_memory() {
    setup_security_env();
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("mem.db");
    let summarizer = Arc::new(SummarizerMock);
    let mut config = SqliteMemoryProviderConfig::new(db_path.to_str().unwrap());
    config.summarizer = Some(summarizer);
    config.summary_threshold = Some(999);
    let provider = SqliteMemoryProvider::new(config).await.unwrap();

    provider
        .append_message("u1", "user", "I like ButterFly Bot")
        .await
        .unwrap();
    provider
        .append_message("u1", "assistant", "Noted")
        .await
        .unwrap();

    provider.summarize_now("u1").await.unwrap();

    let results = provider.search("u1", "ButterFly Bot", 5).await.unwrap();
    assert!(!results.is_empty());
}
