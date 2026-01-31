mod common;

use serde_json::json;

use butterfly_bot::config::{AgentConfig, Config, GuardrailConfig, GuardrailsConfig, OpenAiConfig};
use butterfly_bot::error::ButterflyBotError;
use butterfly_bot::factories::agent_factory::ButterflyBotFactory;

#[tokio::test]
async fn config_from_file_and_factory_errors() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        json!({
            "openai": {"api_key":"key","model":null,"base_url":null},
            "agents": [{
                "name":"agent",
                "instructions":"inst",
                "specialization":"spec",
                "description":null,
                "capture_name":null,
                "capture_schema":null
            }],
            "business": null,
            "guardrails": null
        })
        .to_string(),
    )
    .unwrap();
    let config = Config::from_file(tmp.path()).unwrap();
    let _ = ButterflyBotFactory::create_from_config(config)
        .await
        .unwrap();

    let no_key_with_base_url = Config {
        openai: Some(OpenAiConfig {
            api_key: None,
            model: None,
            base_url: Some("http://localhost:11434/v1".to_string()),
        }),
        agents: vec![AgentConfig {
            name: "agent".to_string(),
            instructions: "inst".to_string(),
            specialization: "spec".to_string(),
            description: None,
            tools: None,
            capture_name: None,
            capture_schema: None,
        }],
        business: None,
        memory: None,
        guardrails: None,
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
        agents: vec![AgentConfig {
            name: "agent".to_string(),
            instructions: "inst".to_string(),
            specialization: "spec".to_string(),
            description: None,
            tools: None,
            capture_name: None,
            capture_schema: None,
        }],
        business: None,
        memory: None,
        guardrails: None,
        tools: None,
        brains: None,
    };
    let err = ButterflyBotFactory::create_from_config(missing_key)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let bad = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(bad.path(), "{bad}").unwrap();
    let err = Config::from_file(bad.path()).unwrap_err();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let err = Config::from_file("/nope/not-found.json").unwrap_err();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let missing = Config {
        openai: None,
        agents: Vec::new(),
        business: None,
        memory: None,
        guardrails: None,
        tools: None,
        brains: None,
    };
    let err = ButterflyBotFactory::create_from_config(missing)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, ButterflyBotError::Config(_)));

    let guardrails = Config {
        openai: Some(OpenAiConfig {
            api_key: Some("key".to_string()),
            model: None,
            base_url: None,
        }),
        agents: vec![AgentConfig {
            name: "agent".to_string(),
            instructions: "inst".to_string(),
            specialization: "spec".to_string(),
            description: None,
            tools: None,
            capture_name: None,
            capture_schema: None,
        }],
        business: None,
        memory: None,
        guardrails: Some(GuardrailsConfig {
            input: Some(vec![GuardrailConfig {
                class: "noop".to_string(),
                config: None,
            }]),
            output: Some(vec![GuardrailConfig {
                class: "noop".to_string(),
                config: None,
            }]),
        }),
        tools: None,
        brains: None,
    };
    let _ = ButterflyBotFactory::create_from_config(guardrails)
        .await
        .unwrap();

    let mixed_guardrails = Config {
        openai: Some(OpenAiConfig {
            api_key: Some("key".to_string()),
            model: None,
            base_url: None,
        }),
        agents: vec![AgentConfig {
            name: "agent".to_string(),
            instructions: "inst".to_string(),
            specialization: "spec".to_string(),
            description: None,
            tools: None,
            capture_name: None,
            capture_schema: None,
        }],
        business: None,
        memory: None,
        guardrails: Some(GuardrailsConfig {
            input: Some(vec![
                GuardrailConfig {
                    class: "PII".to_string(),
                    config: None,
                },
                GuardrailConfig {
                    class: "noop".to_string(),
                    config: None,
                },
            ]),
            output: Some(vec![GuardrailConfig {
                class: "PII".to_string(),
                config: None,
            }]),
        }),
        tools: None,
        brains: None,
    };
    let _ = ButterflyBotFactory::create_from_config(mixed_guardrails)
        .await
        .unwrap();

    let _ok: butterfly_bot::error::Result<()> = Ok(());
    let err = ButterflyBotError::Runtime("boom".to_string());
    assert!(format!("{err}").contains("boom"));
}
