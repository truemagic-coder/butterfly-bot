mod common;

use serde_json::json;

use butterfly_bot::config::{Config, MarkdownSource, OpenAiConfig};
use butterfly_bot::error::ButterflyBotError;
use butterfly_bot::factories::agent_factory::ButterflyBotFactory;

#[tokio::test]
async fn config_from_file_and_factory_errors() {
    let temp = tempfile::tempdir().expect("temp dir");
    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(temp.path().to_path_buf()));
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "config-factory-test-dek".to_string(),
    ));

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
        provider: None,
        openai: Some(OpenAiConfig {
            api_key: Some("dummy".to_string()),
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
        provider: None,
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
        provider: None,
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

    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(None);
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(None);
    butterfly_bot::runtime_paths::set_debug_app_root_override(None);
}
