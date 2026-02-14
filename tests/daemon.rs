use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use httpmock::Method::POST;
use httpmock::MockServer;
use serde_json::json;
use tempfile::NamedTempFile;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use butterfly_bot::client::ButterflyBot;
use butterfly_bot::config::{Config, OpenAiConfig};
use butterfly_bot::daemon::{build_router, AppState};
use butterfly_bot::reminders::ReminderStore;

async fn make_agent(server: &MockServer) -> ButterflyBot {
    let config = Config {
        openai: Some(OpenAiConfig {
            api_key: Some("key".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            base_url: Some(server.base_url()),
        }),
        skill_file: None,
        heartbeat_file: None,
        prompt_file: None,
        memory: None,
        tools: None,
        brains: None,
    };

    ButterflyBot::from_config(config).await.unwrap()
}

#[tokio::test]
async fn daemon_health_and_auth() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let reminder_db = NamedTempFile::new().unwrap();
    let reminder_store = ReminderStore::new(reminder_db.path().to_str().unwrap())
        .await
        .unwrap();
    let db_path = reminder_db.path().to_str().unwrap().to_string();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
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
    let reminder_db = NamedTempFile::new().unwrap();
    let reminder_store = ReminderStore::new(reminder_db.path().to_str().unwrap())
        .await
        .unwrap();
    let db_path = reminder_db.path().to_str().unwrap().to_string();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
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
    chat_mock.assert_hits(1);

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
}

#[tokio::test]
async fn daemon_doctor_requires_auth_and_returns_checks() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let reminder_db = NamedTempFile::new().unwrap();
    let reminder_store = ReminderStore::new(reminder_db.path().to_str().unwrap())
        .await
        .unwrap();
    let db_path = reminder_db.path().to_str().unwrap().to_string();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
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
    assert!(has_db_check, "expected database_access check in doctor output");
}

#[tokio::test]
async fn daemon_security_audit_requires_auth_and_returns_findings() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let reminder_db = NamedTempFile::new().unwrap();
    let reminder_store = ReminderStore::new(reminder_db.path().to_str().unwrap())
        .await
        .unwrap();
    let db_path = reminder_db.path().to_str().unwrap().to_string();
    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
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
