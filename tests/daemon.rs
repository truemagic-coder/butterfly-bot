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
use butterfly_bot::inbox_state::InboxStateStore;
use butterfly_bot::planning::PlanStore;
use butterfly_bot::reminders::ReminderStore;
use butterfly_bot::tasks::TaskStore;
use butterfly_bot::todo::TodoStore;

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
async fn daemon_inbox_and_actionable_count() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-inbox.db");
    let db_path = db_file.to_string_lossy().to_string();

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let reminder = reminder_store
        .create_reminder("u", "Pay electricity bill", now + 120)
        .await
        .unwrap();

    let todo_store = TodoStore::new(&db_path).await.unwrap();
    let _todo_open = todo_store
        .create_item("u", "Call insurance", Some("Policy renewal"), None)
        .await
        .unwrap();
    let todo_done = todo_store
        .create_item("u", "Archive docs", None, None)
        .await
        .unwrap();
    let _ = todo_store.set_completed(todo_done.id, true).await.unwrap();

    let task_store = TaskStore::new(&db_path).await.unwrap();
    let _task = task_store
        .create_task("u", "Weekly check-in", "send summary", now + 300, Some(60))
        .await
        .unwrap();

    let plan_store = PlanStore::new(&db_path).await.unwrap();
    let _plan = plan_store
        .create_plan(
            "u",
            "Hiring plan",
            "Close backend role",
            Some(&json!([
                {"id": "P1", "title": "Review applicants - T-Shirt Size: M, Story Points: 5, Time Estimate: 1 week, Due Date: 2026-03-01", "owner": "human", "status": "new", "priority": "high"},
                {"id": "P2", "title": "Generate shortlist", "owner": "agent", "status": "done", "depends_on": ["P1"]}
            ])),
            Some("active"),
        )
        .await
        .unwrap();

    let (ui_event_tx, _) = broadcast::channel(32);
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
                .uri("/inbox?user_id=u&limit=100&include_done=true")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    assert!(items.iter().any(|item| {
        item.get("source_type").and_then(|v| v.as_str()) == Some("reminder")
            && item.get("source_id").and_then(|v| v.as_i64()) == Some(reminder.id as i64)
    }));
    assert!(items
        .iter()
        .any(|item| item.get("source_type").and_then(|v| v.as_str()) == Some("todo")));
    assert!(items
        .iter()
        .any(|item| item.get("source_type").and_then(|v| v.as_str()) == Some("task")));
    assert!(items
        .iter()
        .any(|item| item.get("source_type").and_then(|v| v.as_str()) == Some("plan_step")));

    let parsed_plan_step = items
        .iter()
        .find(|item| {
            item.get("source_type").and_then(|v| v.as_str()) == Some("plan_step")
                && item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .contains("Review applicants")
        })
        .expect("expected parsed plan step");
    let parsed_due_at = parsed_plan_step
        .get("due_at")
        .and_then(|v| v.as_i64())
        .unwrap_or_default();
    let expected_utc_midnight = 1_772_323_200i64;
    assert!(parsed_due_at > 0);
    assert!((parsed_due_at - expected_utc_midnight).abs() <= 14 * 60 * 60);
    assert_eq!(
        parsed_plan_step
            .get("story_points")
            .and_then(|v| v.as_i64())
            .unwrap_or_default(),
        5
    );
    assert_eq!(
        parsed_plan_step
            .get("t_shirt_size")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "M"
    );
    let expected_dependency_ref = parsed_plan_step
        .get("origin_ref")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let shortlist_step = items
        .iter()
        .find(|item| {
            item.get("source_type").and_then(|v| v.as_str()) == Some("plan_step")
                && item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .contains("Generate shortlist")
        })
        .expect("expected shortlist step");
    let shortlist_deps = shortlist_step
        .get("dependency_refs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        shortlist_deps
            .iter()
            .any(|v| v.as_str() == Some(expected_dependency_ref.as_str())),
        "expected shortlist step to depend on {}",
        expected_dependency_ref
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/inbox/actionable_count?user_id=u")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let actionable_count = value
        .get("actionable_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(actionable_count, 4);
}

#[tokio::test]
async fn daemon_plan_dependency_refs_are_relational_and_visible_in_inbox() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-plan-deps.db");
    let db_path = db_file.to_string_lossy().to_string();

    let plan_store = PlanStore::new(&db_path).await.unwrap();
    let plan = plan_store
        .create_plan(
            "u",
            "Dependency plan",
            "Wire dependency graph",
            Some(&json!([
                {"id": "A1", "title": "Prepare schema", "owner": "human"},
                {"id": "A2", "title": "Hook UI", "owner": "agent", "dependency_refs": ["A1"]}
            ])),
            Some("active"),
        )
        .await
        .unwrap();

    let dep_map = plan_store
        .list_step_dependencies_for_plans(&[plan.id])
        .await
        .unwrap();
    let dependent_step_ref = format!("plan_step:{}:{}", plan.id, 1);
    let required_step_ref = format!("plan_step:{}:{}", plan.id, 0);
    let relational_deps = dep_map
        .get(&dependent_step_ref)
        .cloned()
        .unwrap_or_default();
    assert!(
        relational_deps
            .iter()
            .any(|dep| dep == &required_step_ref.to_ascii_lowercase()),
        "expected relational dependency edge {} -> {}",
        dependent_step_ref,
        required_step_ref
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

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/inbox?user_id=u&limit=100&include_done=true")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let dependent_step = items
        .iter()
        .find(|item| item.get("origin_ref").and_then(|v| v.as_str()) == Some(&dependent_step_ref))
        .expect("expected dependent plan step in inbox");
    let inbox_deps = dependent_step
        .get("dependency_refs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        inbox_deps
            .iter()
            .any(|dep| dep.as_str() == Some(required_step_ref.as_str())),
        "expected inbox dependency edge {} -> {}",
        dependent_step_ref,
        required_step_ref
    );
}

#[tokio::test]
async fn daemon_plan_dependency_title_aliases_resolve_in_inbox() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-plan-title-alias-deps.db");
    let db_path = db_file.to_string_lossy().to_string();

    let plan_store = PlanStore::new(&db_path).await.unwrap();
    let plan = plan_store
        .create_plan(
            "u",
            "Food delivery plan",
            "Launch service",
            Some(&json!([
                {"title": "Conduct market research", "owner": "human"},
                {"title": "Develop business plan", "owner": "human", "depends_on": "market research"}
            ])),
            Some("active"),
        )
        .await
        .unwrap();

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
                .uri("/inbox?user_id=u&limit=100&include_done=true")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let dependent_ref = format!("plan_step:{}:{}", plan.id, 1);
    let prerequisite_ref = format!("plan_step:{}:{}", plan.id, 0);
    let dependent_item = items
        .iter()
        .find(|item| item.get("origin_ref").and_then(|v| v.as_str()) == Some(&dependent_ref))
        .expect("expected dependent plan step in inbox");
    let deps = dependent_item
        .get("dependency_refs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        deps.iter()
            .any(|value| value.as_str() == Some(prerequisite_ref.as_str())),
        "expected '{}' to resolve to '{}'",
        dependent_ref,
        prerequisite_ref
    );
}

#[tokio::test]
async fn daemon_inbox_parses_todo_dependencies_from_notes_fallback() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-todo-notes-deps.db");
    let db_path = db_file.to_string_lossy().to_string();

    let todo_store = TodoStore::new(&db_path).await.unwrap();
    let prereq = todo_store
        .create_item("u", "Prerequisite todo", Some("first"), None)
        .await
        .unwrap();
    let dependent_notes = format!("Depends On: todo:{}", prereq.id);
    let _dependent = todo_store
        .create_item("u", "Dependent todo", Some(&dependent_notes), None)
        .await
        .unwrap();

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
                .uri("/inbox?user_id=u&limit=100&include_done=true")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let dependent_item = items
        .iter()
        .find(|item| {
            item.get("source_type").and_then(|v| v.as_str()) == Some("todo")
                && item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .contains("Dependent todo")
        })
        .expect("expected dependent todo item");
    let deps = dependent_item
        .get("dependency_refs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        deps.iter()
            .any(|value| value.as_str() == Some(format!("todo:{}", prereq.id).as_str())),
        "expected notes dependency to be visible in inbox"
    );
}

#[tokio::test]
async fn daemon_clear_user_data_endpoint() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-clear-user-data.db");
    let db_path = db_file.to_string_lossy().to_string();

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let reminder = reminder_store
        .create_reminder("u", "Call dentist", now + 300)
        .await
        .unwrap();

    let todo_store = TodoStore::new(&db_path).await.unwrap();
    let _todo = todo_store
        .create_item("u", "Pack bag", Some("for trip"), None)
        .await
        .unwrap();

    let task_store = TaskStore::new(&db_path).await.unwrap();
    let _task = task_store
        .create_task("u", "Daily recap", "Write recap", now + 60, None)
        .await
        .unwrap();

    let plan_store = PlanStore::new(&db_path).await.unwrap();
    let _plan = plan_store
        .create_plan("u", "Launch", "ship release", None, Some("active"))
        .await
        .unwrap();

    let inbox_state_store = InboxStateStore::new(&db_path).await.unwrap();
    inbox_state_store
        .set_status("u", &format!("reminder:{}", reminder.id), "acknowledged")
        .await
        .unwrap();

    let (ui_event_tx, _) = broadcast::channel(16);
    let state = AppState {
        agent: Arc::new(RwLock::new(Arc::new(agent))),
        reminder_store: Arc::new(reminder_store),
        signer_service: butterfly_bot::security::signer_daemon::SignerService::default(),
        token: "token".to_string(),
        ui_event_tx,
        db_path: db_path.clone(),
    };
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/clear_user_data")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"user_id":"u"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let cleared = value.get("cleared").cloned().unwrap_or_default();
    assert!(
        cleared
            .get("reminders")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            >= 1
    );
    assert!(cleared.get("todos").and_then(|v| v.as_u64()).unwrap_or(0) >= 1);
    assert!(cleared.get("tasks").and_then(|v| v.as_u64()).unwrap_or(0) >= 1);
    assert!(cleared.get("plans").and_then(|v| v.as_u64()).unwrap_or(0) >= 1);
    assert!(
        cleared
            .get("inbox_state_overrides")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            >= 1
    );

    let reminders = ReminderStore::new(&db_path)
        .await
        .unwrap()
        .list_reminders("u", butterfly_bot::reminders::ReminderStatus::All, 50)
        .await
        .unwrap();
    assert!(reminders.is_empty());

    let todos = TodoStore::new(&db_path)
        .await
        .unwrap()
        .list_items("u", butterfly_bot::todo::TodoStatus::All, 50)
        .await
        .unwrap();
    assert!(todos.is_empty());
}

#[tokio::test]
async fn daemon_reminder_delivery_events_endpoint() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-reminder-events.db");
    let db_path = db_file.to_string_lossy().to_string();
    let audit_log_path = temp.path().join("reminders_audit.log");

    let cfg = Config {
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
            "reminders": {
                "sqlite_path": db_path,
                "audit_log_path": audit_log_path.to_string_lossy().to_string()
            }
        })),
        brains: None,
    };
    config_store::save_config(&db_path, &cfg).unwrap();

    let line_a = json!({
        "timestamp": 1,
        "user_id": "u",
        "reminder_id": 11,
        "status": "delivery_attempted",
        "payload": {"title": "A"}
    })
    .to_string();
    let line_b = json!({
        "timestamp": 2,
        "user_id": "u",
        "reminder_id": 11,
        "status": "delivered",
        "payload": {"title": "A"}
    })
    .to_string();
    std::fs::write(&audit_log_path, format!("{line_a}\n{line_b}\n")).unwrap();

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(32);
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
                .uri("/reminders/delivery_events?user_id=u&limit=10")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let events = value
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1].get("status").and_then(|v| v.as_str()),
        Some("delivered")
    );
}

#[tokio::test]
async fn daemon_inbox_transition_persists_status() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-inbox-transition.db");
    let db_path = db_file.to_string_lossy().to_string();

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let reminder = reminder_store
        .create_reminder("u", "Stateful transition reminder", now + 60)
        .await
        .unwrap();

    let (ui_event_tx, _) = broadcast::channel(32);
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
                .uri("/inbox/transition")
                .header("authorization", "Bearer token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "user_id": "u",
                        "origin_ref": format!("reminder:{}", reminder.id),
                        "action": "acknowledge"
                    })
                    .to_string(),
                ))
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
                .uri("/inbox?user_id=u&limit=100&include_done=true")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let expected_origin = format!("reminder:{}", reminder.id);
    let transitioned = items
        .iter()
        .find(|item| {
            item.get("origin_ref").and_then(|v| v.as_str()) == Some(expected_origin.as_str())
        })
        .cloned();
    assert!(transitioned.is_some());
    let transitioned = transitioned.unwrap();
    assert_eq!(
        transitioned.get("status").and_then(|v| v.as_str()),
        Some("acknowledged")
    );
}

#[tokio::test]
async fn daemon_audit_events_endpoint() {
    let server = MockServer::start_async().await;
    let agent = make_agent(&server).await;
    let temp = tempdir().unwrap();
    let db_file = temp.path().join("daemon-audit-events.db");
    let db_path = db_file.to_string_lossy().to_string();
    let audit_log_path = temp.path().join("ui_events.log");

    let cfg = Config {
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
            "settings": {
                "ui_event_log_path": audit_log_path.to_string_lossy().to_string()
            }
        })),
        brains: None,
    };
    config_store::save_config(&db_path, &cfg).unwrap();

    let line_a = json!({
        "timestamp": 10,
        "event_type": "inbox_transition",
        "user_id": "u",
        "tool": "inbox",
        "status": "acknowledged"
    })
    .to_string();
    let line_b = json!({
        "timestamp": 11,
        "event_type": "tasks",
        "user_id": "u",
        "tool": "tasks",
        "status": "ok"
    })
    .to_string();
    std::fs::write(&audit_log_path, format!("{line_a}\n{line_b}\n")).unwrap();

    let reminder_store = ReminderStore::new(&db_path).await.unwrap();
    let (ui_event_tx, _) = broadcast::channel(32);
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
                .uri("/audit/events?user_id=u&limit=10")
                .header("authorization", "Bearer token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let events = value
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].get("event_type").and_then(|v| v.as_str()),
        Some("inbox_transition")
    );
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
