use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use httpmock::Method::POST;
use httpmock::MockServer;
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use butterfly_bot::client::ButterflyBot;
use butterfly_bot::config::{Config, MarkdownSource, OpenAiConfig};
use butterfly_bot::config_store;
use butterfly_bot::daemon::{build_router, AppState};
use butterfly_bot::reminders::ReminderStore;

fn test_app_root() -> std::path::PathBuf {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("butterfly-daemon-tests-root-{unique}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    })
    .clone()
}

async fn make_agent(server: &MockServer) -> ButterflyBot {
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "daemon-test-dek-passphrase".to_string(),
    ));
    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(test_app_root()));
    butterfly_bot::vault::set_secret("db_encryption_key", "daemon-test-sqlcipher-key")
        .expect("set deterministic test db key");

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("butterfly-daemon-test-{unique}"));
    std::fs::create_dir_all(&base).unwrap();

    let config = Config {
        provider: None,
        openai: Some(OpenAiConfig {
            api_key: Some("key".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            base_url: Some(server.base_url()),
        }),
        heartbeat_source: MarkdownSource::default_heartbeat(),
        prompt_source: MarkdownSource::default_prompt(),
        memory: None,
        tools: Some(json!({
            "reminders": {"sqlite_path": base.join("reminders.db").to_string_lossy().to_string()},
            "tasks": {"sqlite_path": base.join("tasks.db").to_string_lossy().to_string()},
            "planning": {"sqlite_path": base.join("planning.db").to_string_lossy().to_string()},
            "todo": {"sqlite_path": base.join("todo.db").to_string_lossy().to_string()},
            "wakeup": {"sqlite_path": base.join("wakeup.db").to_string_lossy().to_string()}
        })),
        brains: None,
    };

    ButterflyBot::from_config(config).await.unwrap()
}

#[tokio::test]
async fn daemon_health_and_auth() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-health.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/process_text")
                .header("content-type", "application/json")
                .body(Body::from(json!({"user_id":"u","text":"hi"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn daemon_empty_token_fails_closed() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-empty-token.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/security_audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn daemon_process_text_and_memory_search() {
    let server = MockServer::start_async().await;
    let chat_mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4o-mini",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hello"},
                    "finish_reason": "stop"
                }]
            }));
        })
        .await;

    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-process.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/process_text")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"user_id":"u","text":"hello"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value.get("text").and_then(|v| v.as_str()), Some("hello"));
    chat_mock.assert_calls(1);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory_search")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"user_id":"u","query":"hello","limit":2}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("results").and_then(|v| v.as_array()).is_some());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/clear_user_history")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"user_id":"u"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/chat_history?user_id=u&limit=10")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let history = value
        .get("history")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(history.is_empty());
}

#[tokio::test]
async fn daemon_doctor_requires_auth_and_returns_checks() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-doctor.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/doctor")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/doctor")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("overall").and_then(|v| v.as_str()).is_some());

    let checks = value
        .get("checks")
        .and_then(|v| v.as_array())
        .expect("checks array");
    assert!(!checks.is_empty());

    let has_db_check = checks.iter().any(|entry| {
        entry
            .get("name")
            .and_then(|v| v.as_str())
            .map(|name| name == "database_access")
            .unwrap_or(false)
    });
    assert!(
        has_db_check,
        "expected database_access check in doctor output"
    );
}

#[tokio::test]
async fn daemon_security_audit_requires_auth_and_returns_findings() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-security-audit.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/security_audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/security_audit")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("overall").and_then(|v| v.as_str()).is_some());

    let findings = value
        .get("findings")
        .and_then(|v| v.as_array())
        .expect("findings array");
    assert!(!findings.is_empty());

    let has_token_finding = findings.iter().any(|entry| {
        entry
            .get("id")
            .and_then(|v| v.as_str())
            .map(|id| id == "daemon_auth_token")
            .unwrap_or(false)
    });
    assert!(
        has_token_finding,
        "expected daemon_auth_token finding in security audit output"
    );
}

#[tokio::test]
async fn daemon_factory_reset_requires_auth_and_returns_default_config() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-factory-reset.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/factory_reset_config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/factory_reset_config")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value.get("status").and_then(|v| v.as_str()), Some("ok"));
    assert!(value.get("config").is_some());

    let openai_base_url = value
        .get("config")
        .and_then(|cfg| cfg.get("openai"))
        .and_then(|openai| openai.get("base_url"))
        .and_then(|base_url| base_url.as_str());
    assert_eq!(openai_base_url, Some("https://api.openai.com/v1"));

    let prompt_source_type = value
        .get("config")
        .and_then(|cfg| cfg.get("prompt_source"))
        .and_then(|source| source.get("type"))
        .and_then(|kind| kind.as_str());
    assert_eq!(
        prompt_source_type,
        Some("database"),
        "expected prompt_source to default to database"
    );
}

#[tokio::test]
async fn daemon_signer_endpoints_enforce_auth_and_transitions() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-signer.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "req-auth",
                        "actor": "agent",
                        "user_id": "u1",
                        "action_type": "x402_payment",
                        "amount_atomic": 100,
                        "payee": "merchant.local",
                        "context_requires_approval": false,
                        "scheme_id": "v2-solana-exact",
                        "chain_id": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                        "payment_authority": "https://merchant.local"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let preview = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/preview")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "req-approve",
                        "actor": "agent",
                        "user_id": "u1",
                        "action_type": "x402_payment",
                        "amount_atomic": 100,
                        "payee": "merchant.local",
                        "context_requires_approval": true,
                        "scheme_id": "v2-solana-exact",
                        "chain_id": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                        "payment_authority": "https://merchant.local"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preview.status(), StatusCode::OK);

    let approve = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/approve")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"request_id":"req-approve"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::OK);

    let sign = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/sign")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"request_id":"req-approve"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sign.status(), StatusCode::OK);

    let denied_transition = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/sign")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"request_id":"req-missing"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_transition.status(), StatusCode::FORBIDDEN);

    let preview_for_deny = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/preview")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "req-deny",
                        "actor": "agent",
                        "user_id": "u1",
                        "action_type": "x402_payment",
                        "amount_atomic": 100,
                        "payee": "merchant.local",
                        "context_requires_approval": true,
                        "scheme_id": "v2-solana-exact",
                        "chain_id": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                        "payment_authority": "https://merchant.local"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preview_for_deny.status(), StatusCode::OK);

    let deny = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/deny")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"request_id":"req-deny"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deny.status(), StatusCode::OK);

    let denied_sign_after_deny = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/signer/sign")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"request_id":"req-deny"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_sign_after_deny.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn daemon_x_api_key_auth_and_reload_config_workflow() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-reload-config.db");
    let db_path = db_file.to_string_lossy().to_string();

    let config = Config::convention_defaults(&db_path);
    config_store::save_config(&db_path, &config).expect("save config for reload");

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reload_config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let reloaded = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/reload_config")
                .header("x-api-key", "token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reloaded.status(), StatusCode::OK);

    let doctor = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/doctor")
                .header("x-api-key", "token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(doctor.status(), StatusCode::OK);
}

#[tokio::test]
async fn daemon_preload_boot_emits_boot_events() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-preload.db");
    let db_path = db_file.to_string_lossy().to_string();

    let config = Config::convention_defaults(&db_path);
    config_store::save_config(&db_path, &config).expect("save config for preload");

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, mut ui_event_rx) = broadcast::channel(64);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/preload_boot")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"user_id":"boot-user"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut saw_boot_event = false;
    for _ in 0..10 {
        let evt = tokio::time::timeout(std::time::Duration::from_secs(2), ui_event_rx.recv())
            .await
            .expect("boot event timeout")
            .expect("boot event receive");
        if evt.event_type == "boot" {
            saw_boot_event = true;
            break;
        }
    }

    assert!(
        saw_boot_event,
        "expected preload_boot workflow to emit at least one boot event"
    );
}

#[tokio::test]
async fn daemon_x402_preview_enforces_auth_and_solana_only() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-x402.db");
    let db_path = db_file.to_string_lossy().to_string();
    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/x402/preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "x402-auth",
                        "actor": "agent",
                        "user_id": "u1",
                        "payment_required": {
                            "x402Version": 2,
                            "resource": {
                                "description": "pay",
                                "mimeType": "application/json",
                                "url": "https://merchant.local/pay"
                            },
                            "accepts": [{
                                "scheme": "exact",
                                "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                                "amount": "1000",
                                "payTo": "merchant.local",
                                "maxTimeoutSeconds": 300,
                                "asset": "USDC"
                            }]
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/x402/preview")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "x402-solana-ok",
                        "actor": "agent",
                        "user_id": "u1",
                        "merchant_origin": "https://merchant.local",
                        "payment_required": {
                            "x402Version": 2,
                            "resource": {
                                "description": "pay",
                                "mimeType": "application/json",
                                "url": "https://merchant.local/pay"
                            },
                            "accepts": [{
                                "scheme": "exact",
                                "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                                "amount": "1000",
                                "payTo": "merchant.local",
                                "maxTimeoutSeconds": 300,
                                "asset": "USDC"
                            }]
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);

    let non_solana = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/x402/preview")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "x402-non-solana",
                        "actor": "agent",
                        "user_id": "u1",
                        "payment_required": {
                            "x402Version": 2,
                            "resource": {
                                "description": "pay",
                                "mimeType": "application/json",
                                "url": "https://merchant.local/pay"
                            },
                            "accepts": [{
                                "scheme": "exact",
                                "network": "eip155:8453",
                                "amount": "1000",
                                "payTo": "merchant.local",
                                "maxTimeoutSeconds": 300,
                                "asset": "USDC"
                            }]
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_solana.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn daemon_solana_api_surface_and_rpc_policy_wiring() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;

    let rpc = MockServer::start_async().await;
    let get_balance = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getBalance\"");
            then.status(200).json_body(
                json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":123456789}}),
            );
        })
        .await;
    let get_latest_blockhash = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getLatestBlockhash\"");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":{"context":{"slot":1},"value":{"blockhash":"11111111111111111111111111111111","lastValidBlockHeight":100}}
            }));
        })
        .await;
    let simulate_transaction = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"simulateTransaction\"")
                .body_includes("\"replaceRecentBlockhash\":true");
            then.status(200)
                .json_body(json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":{"err":null,"unitsConsumed":22222}}}));
        })
        .await;
    let send_transaction = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"sendTransaction\"")
                .body_includes("\"skipPreflight\":false");
            then.status(200)
                .json_body(json!({"jsonrpc":"2.0","id":1,"result":"sig-test-123"}));
        })
        .await;
    let signature_status = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getSignatureStatuses\"");
            then.status(200)
                .json_body(json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":[{"confirmationStatus":"confirmed","err":null}]}}));
        })
        .await;
    let history = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getSignaturesForAddress\"");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":[{"signature":"sig-test-123","slot":1,"err":null}]
            }));
        })
        .await;

    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-solana.db");
    let db_path = db_file.to_string_lossy().to_string();

    let mut config = Config::convention_defaults(&db_path);
    let mut tools = config.tools.clone().unwrap_or_else(|| json!({}));
    tools["settings"]["solana"]["rpc"]["endpoint"] = json!(rpc.base_url());
    config.tools = Some(tools);
    config_store::save_config(&db_path, &config).unwrap();
    let loaded = Config::from_store(&db_path).unwrap();
    let loaded_policy =
        butterfly_bot::security::solana_rpc_policy::SolanaRpcExecutionPolicy::from_config(&loaded)
            .unwrap();
    assert_eq!(
        loaded_policy.endpoint.as_deref(),
        Some(rpc.base_url().as_str())
    );

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path,
    };
    let app = build_router(state);

    let wallet = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/solana/wallet?user_id=u1&actor=agent")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wallet.status(), StatusCode::OK);
    let wallet_bytes = wallet.into_body().collect().await.unwrap().to_bytes();
    let wallet_json: serde_json::Value = serde_json::from_slice(&wallet_bytes).unwrap();
    let wallet_address = wallet_json
        .get("address")
        .and_then(|value| value.as_str())
        .unwrap()
        .to_string();

    let balance = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/solana/balance?address={wallet_address}"))
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(balance.status(), StatusCode::OK);

    let simulate = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/solana/simulate_transfer")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "sol-sim-1",
                        "user_id": "u1",
                        "actor": "agent",
                        "to": "11111111111111111111111111111111",
                        "lamports": 1000,
                        "payee": "merchant.local"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(simulate.status(), StatusCode::OK);

    let transfer = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/solana/transfer")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "request_id": "sol-send-1",
                        "user_id": "u1",
                        "actor": "agent",
                        "to": "11111111111111111111111111111111",
                        "lamports": 1000,
                        "payee": "merchant.local"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(transfer.status(), StatusCode::OK);

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/solana/tx/status?signature=sig-test-123")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);

    let history_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/solana/tx/history?address={wallet_address}&limit=5"
                ))
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(history_resp.status(), StatusCode::OK);

    get_balance.assert_calls(1);
    get_latest_blockhash.assert_calls(2);
    simulate_transaction.assert_calls(2);
    send_transaction.assert_calls(1);
    signature_status.assert_calls(1);
    history.assert_calls(1);
}
