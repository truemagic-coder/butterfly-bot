use butterfly_bot::config::{Config, MarkdownSource, OpenAiConfig};
use butterfly_bot::factories::agent_factory::ButterflyBotFactory;
use butterfly_bot::sandbox::ToolRuntime;

#[tokio::test]
async fn all_registered_tools_resolve_to_wasm_runtime() {
    let temp = tempfile::tempdir().expect("temp dir should create");
    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(temp.path().to_path_buf()));
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "wasm-runtime-invariant-test-dek".to_string(),
    ));

    let config = Config {
        openai: Some(OpenAiConfig {
            api_key: Some("test-key".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            base_url: Some("http://localhost:11434/v1".to_string()),
        }),
        heartbeat_source: MarkdownSource::default_heartbeat(),
        prompt_source: MarkdownSource::default_prompt(),
        memory: None,
        tools: None,
        brains: None,
    };

    let query_service = ButterflyBotFactory::create_from_config(config)
        .await
        .expect("query service should build");

    let tool_registry = query_service.agent_service().tool_registry.clone();
    let tool_names = tool_registry.list_all_tools().await;

    assert!(
        !tool_names.is_empty(),
        "expected at least one registered tool"
    );

    for tool_name in tool_names {
        let runtime = tool_registry.resolved_runtime_for_tool(&tool_name).await;
        assert_eq!(
            runtime,
            ToolRuntime::Wasm,
            "tool '{}' resolved to non-WASM runtime",
            tool_name
        );
    }

    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(None);
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(None);
    butterfly_bot::runtime_paths::set_debug_app_root_override(None);
}
