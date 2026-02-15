mod common;

use serde_json::json;

use butterfly_bot::config::{Config, MarkdownSource, OpenAiConfig};
use butterfly_bot::error::ButterflyBotError;
use butterfly_bot::factories::agent_factory::ButterflyBotFactory;

#[tokio::test]
async fn config_from_file_and_factory_errors() {
    let config: Config = serde_json::from_value(json!({
        "openai": {"api_key":"key","model":null,"base_url":null},
        "heartbeat_source": {"type": "database", "markdown": ""},
        "prompt_source": {"type": "database", "markdown": ""}
    }))
    .unwrap();
    let _ = ButterflyBotFactory::create_from_config(config)
        .await
        .unwrap();

    let no_key_with_base_url = Config {
        openai: Some(OpenAiConfig {
            api_key: None,
            model: None,
            base_url: Some("http://localhost:11434/v1".to_string()),
        }),
        heartbeat_source: MarkdownSource::default_heartbeat(),
        prompt_source: MarkdownSource::default_prompt(),
        memory: None,
        tools: None,
        brains: None,
    };
    let _ = ButterflyBotFactory::create_from_config(no_key_with_base_url)
        .await
        .unwrap();

    let missing_key = Config {
        openai: Some(OpenAiConfig {
            api_key: None,
            model: None,
            base_url: None,
        }),
        heartbeat_source: MarkdownSource::default_heartbeat(),
        prompt_source: MarkdownSource::default_prompt(),
        memory: None,
        tools: None,
        brains: None,
    };
    let err = ButterflyBotFactory::create_from_config(missing_key)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let err = serde_json::from_str::<Config>("{bad}")
        .map_err(|e| ButterflyBotError::Config(e.to_string()))
        .unwrap_err();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let missing = Config {
        openai: None,
        heartbeat_source: MarkdownSource::default_heartbeat(),
        prompt_source: MarkdownSource::default_prompt(),
        memory: None,
        tools: None,
        brains: None,
    };
    let err = ButterflyBotFactory::create_from_config(missing)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let _ok: butterfly_bot::error::Result<()> = Ok(());
    let err = ButterflyBotError::Runtime("boom".to_string());
    assert!(format!("{err}").contains("boom"));
}
