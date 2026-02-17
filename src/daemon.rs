use std::future::Future;
use std::io::Write;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::client::ButterflyBot;
use crate::config::Config;
use crate::config_store;
use crate::error::{ButterflyBotError, Result};
use crate::factories::agent_factory::load_markdown_content;
use crate::interfaces::scheduler::ScheduledJob;
use crate::reminders::{resolve_reminder_db_path, ReminderStore};
use crate::sandbox::{SandboxSettings, ToolRuntime};
use crate::scheduler::Scheduler;
use crate::services::agent::UiEvent;
use crate::services::query::{OutputFormat, ProcessOptions, ProcessResult, UserInput};
use crate::tasks::TaskStore;
use crate::vault;
use crate::wakeup::WakeupStore;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<RwLock<Arc<ButterflyBot>>>,
    pub reminder_store: Arc<ReminderStore>,
    pub token: String,
    pub ui_event_tx: broadcast::Sender<UiEvent>,
    pub db_path: String,
}

static AUTONOMY_LAST_RUN_TS: AtomicI64 = AtomicI64::new(0);
static AUTONOMY_COOLDOWN_SECS: AtomicI64 = AtomicI64::new(60);

fn set_autonomy_cooldown_seconds(seconds: u64) {
    AUTONOMY_COOLDOWN_SECS.store(seconds.max(1) as i64, Ordering::Relaxed);
}

fn try_begin_autonomy_tick(now_ts: i64) -> Option<i64> {
    loop {
        let cooldown = AUTONOMY_COOLDOWN_SECS.load(Ordering::Relaxed).max(1);
        let last = AUTONOMY_LAST_RUN_TS.load(Ordering::Relaxed);
        if last > 0 {
            let elapsed = now_ts.saturating_sub(last);
            if elapsed < cooldown {
                return Some(cooldown - elapsed);
            }
        }

        if AUTONOMY_LAST_RUN_TS
            .compare_exchange(last, now_ts, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return None;
        }
    }
}

struct BrainTickJob {
    agent: Arc<RwLock<Arc<ButterflyBot>>>,
    interval: Duration,
}

#[async_trait::async_trait]
impl ScheduledJob for BrainTickJob {
    fn name(&self) -> &str {
        "brain_tick"
    }

    fn interval(&self) -> Duration {
        self.interval
    }

    async fn run(&self) -> Result<()> {
        let agent = self.agent.read().await.clone();
        agent.brain_tick().await;
        Ok(())
    }
}

struct WakeupJob {
    agent: Arc<RwLock<Arc<ButterflyBot>>>,
    store: Arc<WakeupStore>,
    interval: Duration,
    ui_event_tx: broadcast::Sender<UiEvent>,
    audit_log_path: Option<String>,
    heartbeat_source: crate::config::MarkdownSource,
    db_path: String,
}

struct ScheduledTasksJob {
    agent: Arc<RwLock<Arc<ButterflyBot>>>,
    store: Arc<TaskStore>,
    interval: Duration,
    ui_event_tx: broadcast::Sender<UiEvent>,
    audit_log_path: Option<String>,
}

#[async_trait::async_trait]
impl ScheduledJob for ScheduledTasksJob {
    fn name(&self) -> &str {
        "scheduled_tasks"
    }

    fn interval(&self) -> Duration {
        self.interval
    }

    async fn run(&self) -> Result<()> {
        let now = now_ts();
        let tasks = self.store.list_due(now, 32).await?;
        for task in tasks {
            let agent = self.agent.read().await.clone();
            let run_at = now_ts();
            let next_run_at = if let Some(interval) = task.interval_minutes {
                run_at + interval.max(1) * 60
            } else {
                run_at
            };

            if task.interval_minutes.is_some() {
                let _ = self.store.mark_run(task.id, run_at, next_run_at).await;
            } else {
                let _ = self.store.complete_one_shot(task.id).await;
            }

            let options = ProcessOptions {
                prompt: None,
                images: Vec::new(),
                output_format: OutputFormat::Text,
                image_detail: "auto".to_string(),
                json_schema: None,
            };
            let input = format!("Scheduled task '{}': {}", task.name, task.prompt);
            let result = agent
                .process(&task.user_id, UserInput::Text(input), options)
                .await;

            let (status, payload): (String, serde_json::Value) = match result {
                Ok(ProcessResult::Text(text)) => (
                    "ok".to_string(),
                    json!({"task_id": task.id, "name": task.name, "output": text}),
                ),
                Ok(other) => (
                    "ok".to_string(),
                    json!({"task_id": task.id, "name": task.name, "output": format!("{other:?}")}),
                ),
                Err(err) => (
                    "error".to_string(),
                    json!({"task_id": task.id, "name": task.name, "error": err.to_string()}),
                ),
            };

            let event = UiEvent {
                event_type: "tasks".to_string(),
                user_id: task.user_id.clone(),
                tool: "tasks".to_string(),
                status: status.clone(),
                payload: payload.clone(),
                timestamp: run_at,
            };
            let _ = self.ui_event_tx.send(event);
            let _ = write_tasks_audit_log(
                self.audit_log_path.as_deref(),
                run_at,
                &task,
                status.as_str(),
                payload,
            );
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl ScheduledJob for WakeupJob {
    fn name(&self) -> &str {
        "wakeup"
    }

    fn interval(&self) -> Duration {
        self.interval
    }

    async fn run(&self) -> Result<()> {
        let now = now_ts();
        let dynamic_source = Config::from_store(&self.db_path)
            .ok()
            .map(|cfg| cfg.heartbeat_source)
            .unwrap_or_else(|| self.heartbeat_source.clone());
        let prompt_source = Config::from_store(&self.db_path)
            .ok()
            .map(|cfg| cfg.prompt_source);

        match load_markdown_content(&dynamic_source).await {
            Ok(markdown) => {
                let agent = self.agent.read().await.clone();
                agent.set_heartbeat_markdown(markdown.clone()).await;
                let status = if markdown
                    .as_ref()
                    .map(|m| !m.trim().is_empty())
                    .unwrap_or(false)
                {
                    "ok"
                } else {
                    "empty"
                };
                let event = UiEvent {
                    event_type: "wakeup".to_string(),
                    user_id: "system".to_string(),
                    tool: "heartbeat".to_string(),
                    status: status.to_string(),
                    payload: json!({"source": dynamic_source}),
                    timestamp: now_ts(),
                };
                let _ = self.ui_event_tx.send(event);
            }
            Err(err) => {
                let event = UiEvent {
                    event_type: "wakeup".to_string(),
                    user_id: "system".to_string(),
                    tool: "heartbeat".to_string(),
                    status: "error".to_string(),
                    payload: json!({"source": dynamic_source, "error": err.to_string()}),
                    timestamp: now_ts(),
                };
                let _ = self.ui_event_tx.send(event);
            }
        }

        if let Some(source) = &prompt_source {
            match load_markdown_content(source).await {
                Ok(markdown) => {
                    let agent = self.agent.read().await.clone();
                    agent.set_prompt_markdown(markdown.clone()).await;
                    let status = if markdown
                        .as_ref()
                        .map(|m| !m.trim().is_empty())
                        .unwrap_or(false)
                    {
                        "ok"
                    } else {
                        "empty"
                    };
                    let event = UiEvent {
                        event_type: "wakeup".to_string(),
                        user_id: "system".to_string(),
                        tool: "prompt".to_string(),
                        status: status.to_string(),
                        payload: json!({"source": source}),
                        timestamp: now_ts(),
                    };
                    let _ = self.ui_event_tx.send(event);
                }
                Err(err) => {
                    let event = UiEvent {
                        event_type: "wakeup".to_string(),
                        user_id: "system".to_string(),
                        tool: "prompt".to_string(),
                        status: "error".to_string(),
                        payload: json!({"source": source, "error": err.to_string()}),
                        timestamp: now_ts(),
                    };
                    let _ = self.ui_event_tx.send(event);
                }
            }
        }

        // Autonomous heartbeat processing
        {
            let agent = self.agent.read().await.clone();
            let ui_event_tx = self.ui_event_tx.clone();
            tokio::spawn(async move {
                run_autonomy_tick(agent, ui_event_tx, "system".to_string(), "wakeup").await;
            });
        }

        let tasks = self.store.list_due(now, 32).await?;
        for task in tasks {
            let agent = self.agent.read().await.clone();
            let run_at = now_ts();
            let next_run_at = run_at + task.interval_minutes.max(1) * 60;
            let _ = self.store.mark_run(task.id, run_at, next_run_at).await;

            let options = ProcessOptions {
                prompt: None,
                images: Vec::new(),
                output_format: OutputFormat::Text,
                image_detail: "auto".to_string(),
                json_schema: None,
            };
            let input = format!("Wakeup task '{}': {}", task.name, task.prompt);
            let result = agent
                .process(&task.user_id, UserInput::Text(input), options)
                .await;

            let (status, payload): (String, Value) = match result {
                Ok(ProcessResult::Text(text)) => (
                    "ok".to_string(),
                    json!({"task_id": task.id, "name": task.name, "output": text}),
                ),
                Ok(other) => (
                    "ok".to_string(),
                    json!({"task_id": task.id, "name": task.name, "output": format!("{other:?}")}),
                ),
                Err(err) => (
                    "error".to_string(),
                    json!({"task_id": task.id, "name": task.name, "error": err.to_string()}),
                ),
            };

            let event = UiEvent {
                event_type: "wakeup".to_string(),
                user_id: task.user_id.clone(),
                tool: "wakeup".to_string(),
                status: status.clone(),
                payload: payload.clone(),
                timestamp: run_at,
            };
            let _ = self.ui_event_tx.send(event);
            let _ = write_wakeup_audit_log(
                self.audit_log_path.as_deref(),
                run_at,
                &task,
                status.as_str(),
                payload.clone(),
            );
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Deserialize)]
struct ProcessTextRequest {
    user_id: String,
    text: String,
    prompt: Option<String>,
}

#[derive(Serialize)]
struct ProcessTextResponse {
    text: String,
}

#[derive(Deserialize)]
struct MemorySearchRequest {
    user_id: String,
    query: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ChatHistoryQuery {
    user_id: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ClearHistoryRequest {
    user_id: String,
}

#[derive(Deserialize)]
struct PreloadBootRequest {
    user_id: String,
}

#[derive(Serialize)]
struct PreloadBootResponse {
    context_status: String,
    heartbeat_status: String,
}

#[derive(Deserialize)]
struct ReminderStreamQuery {
    user_id: String,
}

#[derive(Deserialize)]
struct UiEventStreamQuery {
    user_id: Option<String>,
}

#[derive(Serialize)]
struct MemorySearchResponse {
    results: Vec<String>,
}

#[derive(Serialize)]
struct ChatHistoryResponse {
    history: Vec<String>,
}

#[derive(Serialize)]
struct ClearHistoryResponse {
    status: String,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize, Clone)]
struct DoctorCheck {
    name: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
}

#[derive(Serialize)]
struct DoctorResponse {
    overall: String,
    checks: Vec<DoctorCheck>,
}

#[derive(Serialize, Clone)]
struct SecurityAuditFinding {
    id: String,
    severity: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
    auto_fixable: bool,
}

#[derive(Serialize)]
struct SecurityAuditResponse {
    overall: String,
    findings: Vec<SecurityAuditFinding>,
}

#[derive(Serialize)]
struct FactoryResetConfigResponse {
    status: String,
    message: String,
    config: Value,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/doctor", post(doctor))
        .route("/security_audit", post(security_audit))
        .route("/process_text", post(process_text))
        .route("/process_text_stream", post(process_text_stream))
        .route("/chat_history", get(chat_history))
        .route("/clear_user_history", post(clear_user_history))
        .route("/memory_search", post(memory_search))
        .route("/preload_boot", post(preload_boot))
        .route("/reminder_stream", get(reminder_stream))
        .route("/ui_events", get(ui_events))
        .route("/factory_reset_config", post(factory_reset_config))
        .route("/reload_config", post(reload_config))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn doctor(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let checks = run_doctor_checks(&state).await;
    let has_fail = checks.iter().any(|check| check.status == "fail");
    let has_warn = checks.iter().any(|check| check.status == "warn");
    let overall = if has_fail {
        "fail"
    } else if has_warn {
        "warn"
    } else {
        "pass"
    };

    (
        StatusCode::OK,
        Json(DoctorResponse {
            overall: overall.to_string(),
            checks,
        }),
    )
        .into_response()
}

async fn security_audit(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let findings = run_security_audit_checks(&state).await;
    let overall = highest_severity(&findings);

    (
        StatusCode::OK,
        Json(SecurityAuditResponse { overall, findings }),
    )
        .into_response()
}

fn doctor_check(name: &str, status: &str, message: String, fix_hint: Option<&str>) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status: status.to_string(),
        message,
        fix_hint: fix_hint.map(str::to_string),
    }
}

fn security_finding(
    id: &str,
    severity: &str,
    status: &str,
    message: String,
    fix_hint: Option<&str>,
    auto_fixable: bool,
) -> SecurityAuditFinding {
    SecurityAuditFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        status: status.to_string(),
        message,
        fix_hint: fix_hint.map(str::to_string),
        auto_fixable,
    }
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}

fn highest_severity(findings: &[SecurityAuditFinding]) -> String {
    findings
        .iter()
        .filter(|finding| finding.status != "pass")
        .max_by_key(|finding| severity_rank(&finding.severity))
        .map(|finding| finding.severity.clone())
        .unwrap_or_else(|| "low".to_string())
}

async fn run_security_audit_checks(state: &AppState) -> Vec<SecurityAuditFinding> {
    let mut findings = Vec::new();

    if state.token.trim().is_empty() {
        findings.push(security_finding(
            "daemon_auth_token",
            "medium",
            "warn",
            "Daemon auth token is empty; this is unexpected because token bootstrap is automatic and protected endpoints fail closed."
                .to_string(),
            Some("Restart the app/daemon to re-run token bootstrap and verify keyring/secret-store availability."),
            false,
        ));
    } else {
        findings.push(security_finding(
            "daemon_auth_token",
            "low",
            "pass",
            "Daemon auth token is configured.".to_string(),
            None,
            false,
        ));
    }

    match Config::from_store(&state.db_path) {
        Ok(config) => {
            findings.push(security_finding(
                "config_load",
                "low",
                "pass",
                "Config loaded from store/keyring.".to_string(),
                None,
                false,
            ));

            let has_inline_api_key = config
                .openai
                .as_ref()
                .and_then(|openai| openai.api_key.as_ref())
                .map(|key| !key.trim().is_empty())
                .unwrap_or(false)
                || config
                    .memory
                    .as_ref()
                    .and_then(|memory| memory.openai.as_ref())
                    .and_then(|openai| openai.api_key.as_ref())
                    .map(|key| !key.trim().is_empty())
                    .unwrap_or(false);

            if has_inline_api_key {
                findings.push(security_finding(
                    "inline_api_keys",
                    "high",
                    "warn",
                    "API keys appear inline in config JSON; prefer keyring-backed secrets."
                        .to_string(),
                    Some("Remove inline keys and set secrets via `butterfly-bot secrets-set`."),
                    false,
                ));
            } else {
                findings.push(security_finding(
                    "inline_api_keys",
                    "low",
                    "pass",
                    "No inline API keys detected in loaded config.".to_string(),
                    None,
                    false,
                ));
            }

            let root = json!({ "tools": config.tools.clone().unwrap_or(Value::Null) });
            let sandbox = SandboxSettings::from_root_config(&root);

            let built_in_tools = [
                "coding",
                "mcp",
                "http_call",
                "github",
                "zapier",
                "planning",
                "reminders",
                "search_internet",
                "tasks",
                "todo",
                "wakeup",
            ];
            let mut non_wasm_tools = Vec::new();
            for tool_name in built_in_tools {
                let plan = sandbox.execution_plan(tool_name);
                if plan.runtime != ToolRuntime::Wasm {
                    non_wasm_tools.push(tool_name);
                }
            }

            if non_wasm_tools.is_empty() {
                findings.push(security_finding(
                    "tool_runtime_invariant",
                    "low",
                    "pass",
                    "All built-in tools resolve to WASM runtime.".to_string(),
                    None,
                    false,
                ));
            } else {
                findings.push(security_finding(
                    "tool_runtime_invariant",
                    "high",
                    "fail",
                    format!(
                        "Non-WASM tool runtime detected for: {}.",
                        non_wasm_tools.join(", ")
                    ),
                    Some(
                        "Enforce WASM-only execution in sandbox settings and tool runtime planner.",
                    ),
                    false,
                ));
            }

            let default_deny = config
                .tools
                .as_ref()
                .and_then(|tools| tools.get("settings"))
                .and_then(|settings| settings.get("permissions"))
                .and_then(|permissions| permissions.get("default_deny"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

            if default_deny {
                findings.push(security_finding(
                    "network_default_deny",
                    "low",
                    "pass",
                    "Global tools network policy uses default_deny=true.".to_string(),
                    None,
                    false,
                ));
            } else {
                findings.push(security_finding(
                    "network_default_deny",
                    "medium",
                    "warn",
                    "Global tools network policy default_deny is disabled or missing."
                        .to_string(),
                    Some("Set tools.settings.permissions.default_deny to true and allowlist required domains."),
                    false,
                ));
            }
        }
        Err(err) => {
            findings.push(security_finding(
                "config_load",
                "critical",
                "fail",
                format!("Config load failed: {err}"),
                Some("Save a valid config in Config tab and rerun security audit."),
                false,
            ));
        }
    }

    findings
}

async fn run_doctor_checks(state: &AppState) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    if state.token.trim().is_empty() {
        checks.push(doctor_check(
            "daemon_auth_token",
            "warn",
            "Daemon auth token is empty; this is unexpected because token bootstrap is automatic and protected endpoints fail closed."
                .to_string(),
            Some("Restart the app/daemon to re-run token bootstrap and verify keyring/secret-store availability."),
        ));
    } else {
        checks.push(doctor_check(
            "daemon_auth_token",
            "pass",
            "Daemon auth token is configured.".to_string(),
            None,
        ));
    }

    match Config::from_store(&state.db_path) {
        Ok(config) => {
            checks.push(doctor_check(
                "config_store",
                "pass",
                "Config loaded from store/keyring.".to_string(),
                None,
            ));

            match config.clone().resolve_vault() {
                Ok(_) => {
                    checks.push(doctor_check(
                        "vault_resolution",
                        "pass",
                        "Vault-backed secrets resolved successfully.".to_string(),
                        None,
                    ));
                }
                Err(err) => {
                    checks.push(doctor_check(
                        "vault_resolution",
                        "fail",
                        format!("Vault resolution failed: {err}"),
                        Some("Verify OS keychain access and required secret keys."),
                    ));
                }
            }

            match check_provider_health(&config).await {
                Ok(check) => checks.push(check),
                Err(err) => checks.push(doctor_check(
                    "provider_health",
                    "fail",
                    format!("Provider health check failed: {err}"),
                    Some("Verify provider base_url/model and network access."),
                )),
            }
        }
        Err(err) => {
            checks.push(doctor_check(
                "config_store",
                "fail",
                format!("Config load failed: {err}"),
                Some("Save a valid config in the Config tab and retry."),
            ));
            checks.push(doctor_check(
                "vault_resolution",
                "warn",
                "Skipped because config could not be loaded.".to_string(),
                Some("Fix config_store check first."),
            ));
            checks.push(doctor_check(
                "provider_health",
                "warn",
                "Skipped because config could not be loaded.".to_string(),
                Some("Fix config_store check first."),
            ));
        }
    }

    let db_path = state.db_path.clone();
    let db_check = tokio::task::spawn_blocking(move || -> DoctorCheck {
        if let Err(err) = crate::config_store::ensure_parent_dir(&db_path) {
            return doctor_check(
                "database_access",
                "fail",
                format!("Database directory check failed: {err}"),
                Some("Verify filesystem permissions for DB path."),
            );
        }

        let mut conn = match SqliteConnection::establish(&db_path) {
            Ok(conn) => conn,
            Err(err) => {
                return doctor_check(
                    "database_access",
                    "fail",
                    format!("Database open failed: {err}"),
                    Some("Verify DB path and SQLite/SQLCipher availability."),
                )
            }
        };

        if let Err(err) = crate::db::apply_sqlcipher_key_sync(&mut conn) {
            return doctor_check(
                "database_access",
                "fail",
                format!("Database key apply failed: {err}"),
                Some("Verify BUTTERFLY_BOT_DB_KEY or keychain db_encryption_key."),
            );
        }

        let probe_result = diesel::sql_query(
            "CREATE TABLE IF NOT EXISTS doctor_probe (id INTEGER PRIMARY KEY, ts INTEGER NOT NULL)",
        )
        .execute(&mut conn);

        match probe_result {
            Ok(_) => doctor_check(
                "database_access",
                "pass",
                "Database opened and write probe succeeded.".to_string(),
                None,
            ),
            Err(err) => doctor_check(
                "database_access",
                "fail",
                format!("Database write probe failed: {err}"),
                Some("Verify DB permissions and SQLCipher key configuration."),
            ),
        }
    })
    .await;

    match db_check {
        Ok(check) => checks.push(check),
        Err(err) => checks.push(doctor_check(
            "database_access",
            "fail",
            format!("Database check task failed: {err}"),
            Some("Retry diagnostics; if persistent, inspect runtime logs."),
        )),
    }

    checks
}

async fn check_provider_health(config: &Config) -> Result<DoctorCheck> {
    let provider = config.openai.clone().or_else(|| {
        config
            .memory
            .as_ref()
            .and_then(|memory| memory.openai.clone())
    });

    let Some(provider) = provider else {
        return Ok(doctor_check(
            "provider_health",
            "warn",
            "No provider config found in openai or memory.openai.".to_string(),
            Some("Set provider base_url/model in Config tab."),
        ));
    };

    let base_url = provider.base_url.unwrap_or_default();
    if base_url.trim().is_empty() {
        return Ok(doctor_check(
            "provider_health",
            "fail",
            "Provider base_url is empty.".to_string(),
            Some("Set openai.base_url (or memory.openai.base_url)."),
        ));
    }

    let models_url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let result = tokio::time::timeout(Duration::from_secs(3), client.get(&models_url).send()).await;

    match result {
        Ok(Ok(response)) if response.status().is_success() => Ok(doctor_check(
            "provider_health",
            "pass",
            format!("Provider responded successfully at {models_url}"),
            None,
        )),
        Ok(Ok(response)) => Ok(doctor_check(
            "provider_health",
            "warn",
            format!("Provider reachable but returned HTTP {}", response.status()),
            Some("Check provider auth/token and model availability."),
        )),
        Ok(Err(err)) => Ok(doctor_check(
            "provider_health",
            "fail",
            format!("Provider request failed: {err}"),
            Some("Check base_url/network and that provider service is running."),
        )),
        Err(_) => Ok(doctor_check(
            "provider_health",
            "fail",
            "Provider request timed out after 3s.".to_string(),
            Some("Check provider responsiveness and network routing."),
        )),
    }
}

async fn process_text(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ProcessTextRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let options = ProcessOptions {
        prompt: payload.prompt.clone(),
        images: Vec::new(),
        output_format: OutputFormat::Text,
        image_detail: "auto".to_string(),
        json_schema: None,
    };

    let agent = state.agent.read().await.clone();
    let response = agent
        .process(&payload.user_id, UserInput::Text(payload.text), options)
        .await;

    match response {
        Ok(ProcessResult::Text(text)) => {
            (StatusCode::OK, Json(ProcessTextResponse { text })).into_response()
        }
        Ok(other) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Unexpected response: {other:?}"),
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn process_text_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ProcessTextRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let agent = state.agent.read().await.clone();
    let ProcessTextRequest {
        user_id,
        text,
        prompt,
    } = payload;

    let body = Body::from_stream(async_stream::stream! {
        let mut stream = agent.process_text_stream(&user_id, &text, prompt.as_deref());
        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    if !chunk.is_empty() {
                        yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(chunk));
                    }
                }
                Err(err) => {
                    let message = format!("\n[error] {}", err);
                    yield Ok(Bytes::from(message));
                    break;
                }
            }
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .body(body)
        .unwrap()
}

async fn preload_boot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PreloadBootRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let agent = state.agent.read().await.clone();
    let db_path = state.db_path.clone();
    let ui_event_tx = state.ui_event_tx.clone();
    let user_id = payload.user_id.clone();

    tokio::spawn(async move {
        let quick_timeout = Duration::from_secs(2);

        let context_status =
            match tokio::time::timeout(quick_timeout, agent.preload_context(&user_id)).await {
                Ok(Ok(())) => "ok".to_string(),
                Ok(Err(err)) => format!("error: {err}"),
                Err(_) => {
                    let agent = agent.clone();
                    let ui_event_tx = ui_event_tx.clone();
                    let user_id = user_id.clone();
                    tokio::spawn(async move {
                        let status = match agent.preload_context(&user_id).await {
                            Ok(()) => "ok".to_string(),
                            Err(err) => format!("error: {err}"),
                        };
                        let _ = ui_event_tx.send(UiEvent {
                        event_type: "boot".to_string(),
                        user_id: user_id.clone(),
                        tool: "context".to_string(),
                        status: status.clone(),
                        payload: json!({"user_id": user_id, "status": status, "phase": "deferred"}),
                        timestamp: now_ts(),
                    });
                    });
                    "deferred".to_string()
                }
            };
        let _ = ui_event_tx.send(UiEvent {
            event_type: "boot".to_string(),
            user_id: user_id.clone(),
            tool: "context".to_string(),
            status: context_status.clone(),
            payload: json!({"user_id": user_id, "status": context_status, "phase": "quick"}),
            timestamp: now_ts(),
        });

        let heartbeat_status = if let Ok(config) = Config::from_store(&db_path) {
            let source = config.heartbeat_source;
            match tokio::time::timeout(quick_timeout, load_markdown_content(&source)).await {
                Ok(Ok(markdown)) => {
                    agent.set_heartbeat_markdown(markdown.clone()).await;
                    if markdown
                        .as_ref()
                        .map(|m| !m.trim().is_empty())
                        .unwrap_or(false)
                    {
                        "ok".to_string()
                    } else {
                        "empty".to_string()
                    }
                }
                Ok(Err(err)) => format!("error: {err}"),
                Err(_) => {
                    let agent = agent.clone();
                    let ui_event_tx = ui_event_tx.clone();
                    let source = source.clone();
                    tokio::spawn(async move {
                        let status = match load_markdown_content(&source).await {
                            Ok(markdown) => {
                                agent.set_heartbeat_markdown(markdown.clone()).await;
                                if markdown
                                    .as_ref()
                                    .map(|m| !m.trim().is_empty())
                                    .unwrap_or(false)
                                {
                                    "ok".to_string()
                                } else {
                                    "empty".to_string()
                                }
                            }
                            Err(err) => format!("error: {err}"),
                        };
                        let _ = ui_event_tx.send(UiEvent {
                            event_type: "boot".to_string(),
                            user_id: "system".to_string(),
                            tool: "heartbeat".to_string(),
                            status: status.clone(),
                            payload: json!({"status": status, "phase": "deferred"}),
                            timestamp: now_ts(),
                        });
                    });
                    "deferred".to_string()
                }
            }
        } else {
            "config_error".to_string()
        };

        let _ = ui_event_tx.send(UiEvent {
            event_type: "boot".to_string(),
            user_id: "system".to_string(),
            tool: "heartbeat".to_string(),
            status: heartbeat_status.clone(),
            payload: json!({"status": heartbeat_status, "phase": "quick"}),
            timestamp: now_ts(),
        });

        let prompt_status = if let Ok(config) = Config::from_store(&db_path) {
            let source = config.prompt_source;
            match tokio::time::timeout(quick_timeout, load_markdown_content(&source)).await {
                Ok(Ok(markdown)) => {
                    agent.set_prompt_markdown(markdown.clone()).await;
                    if markdown
                        .as_ref()
                        .map(|m| !m.trim().is_empty())
                        .unwrap_or(false)
                    {
                        "ok".to_string()
                    } else {
                        "empty".to_string()
                    }
                }
                Ok(Err(err)) => format!("error: {err}"),
                Err(_) => {
                    let agent = agent.clone();
                    let ui_event_tx = ui_event_tx.clone();
                    let source = source.clone();
                    tokio::spawn(async move {
                        let status = match load_markdown_content(&source).await {
                            Ok(markdown) => {
                                agent.set_prompt_markdown(markdown.clone()).await;
                                if markdown
                                    .as_ref()
                                    .map(|m| !m.trim().is_empty())
                                    .unwrap_or(false)
                                {
                                    "ok".to_string()
                                } else {
                                    "empty".to_string()
                                }
                            }
                            Err(err) => format!("error: {err}"),
                        };
                        let _ = ui_event_tx.send(UiEvent {
                            event_type: "boot".to_string(),
                            user_id: "system".to_string(),
                            tool: "prompt".to_string(),
                            status: status.clone(),
                            payload: json!({"status": status, "phase": "deferred"}),
                            timestamp: now_ts(),
                        });
                    });
                    "deferred".to_string()
                }
            }
        } else {
            "config_error".to_string()
        };

        let _ = ui_event_tx.send(UiEvent {
            event_type: "boot".to_string(),
            user_id: user_id.clone(),
            tool: "prompt".to_string(),
            status: prompt_status.clone(),
            payload: json!({"status": prompt_status}),
            timestamp: now_ts(),
        });

        if (heartbeat_status == "ok"
            || heartbeat_status == "empty"
            || heartbeat_status == "deferred")
            && (prompt_status == "ok" || prompt_status == "empty" || prompt_status == "deferred")
        {
            let agent = agent.clone();
            let ui_event_tx = ui_event_tx.clone();
            let user_id = user_id.clone();
            tokio::spawn(async move {
                run_autonomy_tick(agent, ui_event_tx, user_id, "boot").await;
            });
        }
    });

    (
        StatusCode::OK,
        Json(PreloadBootResponse {
            context_status: "started".to_string(),
            heartbeat_status: "started".to_string(),
        }),
    )
        .into_response()
}

async fn memory_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<MemorySearchRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let limit = payload.limit.unwrap_or(8);
    let agent = state.agent.read().await.clone();
    let response = agent
        .search_memory(&payload.user_id, &payload.query, limit)
        .await;

    match response {
        Ok(results) => (StatusCode::OK, Json(MemorySearchResponse { results })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn chat_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<ChatHistoryQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let limit = query.limit.unwrap_or(40).clamp(1, 200);
    let agent = state.agent.read().await.clone();
    let response = agent.get_user_history(&query.user_id, limit).await;

    match response {
        Ok(history) => (StatusCode::OK, Json(ChatHistoryResponse { history })).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn clear_user_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ClearHistoryRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let agent = state.agent.read().await.clone();
    tracing::info!(
        "clear_user_history requested for user_id={}",
        payload.user_id
    );
    match agent.delete_user_history(&payload.user_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ClearHistoryResponse {
                status: "ok".to_string(),
                message: "User history cleared".to_string(),
            }),
        )
            .into_response(),
        Err(err) => {
            tracing::error!(
                "clear_user_history failed for user_id={}: {}",
                payload.user_id,
                err
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: err.to_string(),
                }),
            )
                .into_response()
        }
    }
}

async fn reminder_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<ReminderStreamQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let store = state.reminder_store.clone();
    let user_id = query.user_id;
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    let body = Body::from_stream(async_stream::stream! {
        loop {
            tick.tick().await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if let Ok(items) = store.due_reminders(&user_id, now, 10).await {
                if (std::env::var("BUTTERFLY_BOT_REMINDER_DEBUG").is_ok()
                    || cfg!(debug_assertions))
                    && !items.is_empty()
                {
                    eprintln!(
                        "Reminder stream emit: user_id={} count={} now={}",
                        user_id,
                        items.len(),
                        now
                    );
                }
                for item in items {
                    let payload = serde_json::json!({
                        "id": item.id,
                        "title": item.title,
                        "due_at": item.due_at,
                    });
                    let line = format!("data: {}\n\n", payload);
                    yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(line));
                }
            }
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap()
}

async fn reload_config(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let agent =
        ButterflyBot::from_store_with_events(&state.db_path, Some(state.ui_event_tx.clone())).await;
    match agent {
        Ok(agent) => {
            let mut guard = state.agent.write().await;
            *guard = Arc::new(agent);
            (
                StatusCode::OK,
                Json(json!({"status": "ok", "message": "Config reloaded"})),
            )
                .into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
            .into_response(),
    }
}

async fn factory_reset_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let config = Config::convention_defaults(&state.db_path);
    if let Err(err) = config_store::save_config(&state.db_path, &config) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
            .into_response();
    }

    let config_value = match serde_json::to_value(&config) {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: err.to_string(),
                }),
            )
                .into_response();
        }
    };

    let pretty = serde_json::to_string_pretty(&config_value).unwrap_or_default();
    let keyring_saved = match vault::set_secret("app_config_json", &pretty) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(
                "factory_reset_config: failed to persist keyring config: {}",
                err
            );
            false
        }
    };

    let mut message = if keyring_saved {
        "Config reset to factory defaults".to_string()
    } else {
        "Config reset to factory defaults (keyring sync failed)".to_string()
    };

    match ButterflyBot::from_store_with_events(&state.db_path, Some(state.ui_event_tx.clone()))
        .await
    {
        Ok(agent) => {
            let mut guard = state.agent.write().await;
            *guard = Arc::new(agent);
        }
        Err(err) => {
            tracing::warn!("factory_reset_config: agent reload failed: {}", err);
            message.push_str("; reload failed, restart daemon to apply runtime state");
        }
    }

    (
        StatusCode::OK,
        Json(FactoryResetConfigResponse {
            status: "ok".to_string(),
            message,
            config: config_value,
        }),
    )
        .into_response()
}

async fn ui_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<UiEventStreamQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize(&headers, &state.token) {
        return err.into_response();
    }

    let mut receiver = state.ui_event_tx.subscribe();
    let filter_user = query.user_id;

    let body = Body::from_stream(async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Some(filter) = &filter_user {
                        if event.user_id != *filter
                            && event.user_id != "system"
                            && event.event_type != "boot"
                            && event.event_type != "autonomy"
                        {
                            continue;
                        }
                    }
                    let payload = serde_json::to_string(&event).unwrap_or_default();
                    let line = format!("data: {}\n\n", payload);
                    yield Ok::<Bytes, std::convert::Infallible>(Bytes::from(line));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(_) => break,
            }
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap()
}

fn authorize(
    headers: &HeaderMap,
    token: &str,
) -> std::result::Result<(), (StatusCode, Json<ErrorResponse>)> {
    let expected_token = token.trim();
    if expected_token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Unauthorized".to_string(),
            }),
        ));
    }

    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let bearer = header.strip_prefix("Bearer ").unwrap_or("").trim();
    let api_key = api_key.trim();

    if bearer == expected_token || api_key == expected_token {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Unauthorized".to_string(),
            }),
        ))
    }
}

pub async fn run(host: &str, port: u16, db_path: &str, token: &str) -> Result<()> {
    run_with_shutdown(host, port, db_path, token, futures::future::pending::<()>()).await
}

pub async fn run_with_shutdown<F>(
    host: &str,
    port: u16,
    db_path: &str,
    token: &str,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    match crate::wasm_bundle::ensure_bundled_wasm_tools() {
        Ok(path) => {
            tracing::info!(
                wasm_dir = %path.to_string_lossy(),
                "Ensured bundled WASM tool modules"
            );
        }
        Err(err) => {
            tracing::warn!("Could not provision bundled WASM tool modules: {}", err);
        }
    }

    if Config::from_store(db_path).is_err() {
        tracing::warn!("No config in store; writing default config for {}", db_path);
        let default_config = Config::convention_defaults(db_path);
        config_store::save_config(db_path, &default_config)?;
    }

    let config = Config::from_store(db_path).ok();

    // ── Log which context/heartbeat source the daemon sees ──
    if let Some(cfg) = &config {
        tracing::info!(
            "Daemon config: prompt_source={:?}, heartbeat_source={:?}",
            cfg.prompt_source,
            cfg.heartbeat_source
        );
    } else {
        tracing::error!("Daemon could not load any config from store!");
    }

    let tick_seconds = config
        .as_ref()
        .and_then(|cfg| cfg.brains.as_ref())
        .and_then(|brains| brains.get("settings"))
        .and_then(|settings| settings.get("tick_seconds"))
        .and_then(|value| value.as_u64())
        .unwrap_or(60);

    let (ui_event_tx, _) = broadcast::channel(256);
    if let Some(path) = ui_event_log_path(config.as_ref()) {
        let mut rx = ui_event_tx.subscribe();
        let path = path.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let _ = write_ui_event_log(&path, &event);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
    let agent = Arc::new(RwLock::new(Arc::new(
        ButterflyBot::from_store_with_events(db_path, Some(ui_event_tx.clone())).await?,
    )));
    let reminder_db_path = config
        .as_ref()
        .and_then(|cfg| serde_json::to_value(cfg).ok())
        .and_then(|value| resolve_reminder_db_path(&value))
        .unwrap_or_else(|| db_path.to_string());
    let reminder_store = Arc::new(ReminderStore::new(reminder_db_path).await?);
    let task_store = Arc::new(TaskStore::new(db_path).await?);
    let wakeup_store = Arc::new(WakeupStore::new(db_path).await?);
    let mut scheduler = Scheduler::new();
    scheduler.register_job(Arc::new(BrainTickJob {
        agent: agent.clone(),
        interval: Duration::from_secs(tick_seconds.max(1)),
    }));
    let wakeup_poll_seconds = config
        .as_ref()
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| tools.get("wakeup"))
        .and_then(|wakeup| wakeup.get("poll_seconds"))
        .and_then(|value| value.as_u64())
        .unwrap_or(60);
    let autonomy_cooldown_seconds = config
        .as_ref()
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| {
            tools
                .get("settings")
                .and_then(|settings| settings.get("autonomy_cooldown_seconds"))
                .and_then(|value| value.as_u64())
                .or_else(|| {
                    tools
                        .get("wakeup")
                        .and_then(|wakeup| wakeup.get("autonomy_cooldown_seconds"))
                        .and_then(|value| value.as_u64())
                })
        })
        .unwrap_or(60);
    set_autonomy_cooldown_seconds(autonomy_cooldown_seconds);
    scheduler.register_job(Arc::new(WakeupJob {
        agent: agent.clone(),
        store: wakeup_store.clone(),
        interval: Duration::from_secs(wakeup_poll_seconds.max(1)),
        ui_event_tx: ui_event_tx.clone(),
        audit_log_path: wakeup_audit_log_path(config.as_ref()),
        heartbeat_source: config
            .as_ref()
            .map(|cfg| cfg.heartbeat_source.clone())
            .unwrap_or_else(crate::config::MarkdownSource::default_heartbeat),
        db_path: db_path.to_string(),
    }));
    let tasks_poll_seconds = config
        .as_ref()
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| tools.get("tasks"))
        .and_then(|tasks| tasks.get("poll_seconds"))
        .and_then(|value| value.as_u64())
        .unwrap_or(60);
    scheduler.register_job(Arc::new(ScheduledTasksJob {
        agent: agent.clone(),
        store: task_store.clone(),
        interval: Duration::from_secs(tasks_poll_seconds.max(1)),
        ui_event_tx: ui_event_tx.clone(),
        audit_log_path: tasks_audit_log_path(config.as_ref()),
    }));
    scheduler.start();

    let state = AppState {
        agent,
        reminder_store,
        token: token.to_string(),
        ui_event_tx,
        db_path: db_path.to_string(),
    };
    let app = build_router(state);

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let shutdown = async move {
        shutdown.await;
        scheduler.stop().await;
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    Ok(())
}

async fn run_autonomy_tick(
    agent: Arc<crate::client::ButterflyBot>,
    ui_event_tx: broadcast::Sender<UiEvent>,
    user_id: String,
    source: &str,
) {
    let run_at = now_ts();
    if let Some(remaining) = try_begin_autonomy_tick(run_at) {
        let _ = ui_event_tx.send(UiEvent {
            event_type: "autonomy".to_string(),
            user_id,
            tool: "heartbeat".to_string(),
            status: "skipped".to_string(),
            payload: json!({
                "source": source,
                "reason": "cooldown",
                "cooldown_remaining_seconds": remaining,
            }),
            timestamp: run_at,
        });
        return;
    }

    let _ = ui_event_tx.send(UiEvent {
        event_type: "autonomy".to_string(),
        user_id: user_id.clone(),
        tool: "heartbeat".to_string(),
        status: "started".to_string(),
        payload: json!({"source": source}),
        timestamp: run_at,
    });

    let options = ProcessOptions {
        prompt: Some(
            "AUTONOMY MODE: Heartbeat tick.\n\
    Run autonomous checks/actions as needed using tools.\n\
    Output requirements:\n\
    - Return ONLY one short final status line (max 120 chars).\n\
    - Do NOT include Thought, Plan, Action, Observation, Summary, or Reasoning sections.\n\
    - Do NOT dump tool call details.\n\
    - Good outputs: 'No-op', 'Processed 2 due tasks', 'Updated plans/todos; no urgent actions'."
                .to_string(),
        ),
        images: Vec::new(),
        output_format: OutputFormat::Text,
        image_detail: "auto".to_string(),
        json_schema: None,
    };
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        agent
            .process(
                &user_id,
                UserInput::Text("Autonomous heartbeat tick".to_string()),
                options,
            )
            .await
    })
    .await;

    let (status, payload): (String, serde_json::Value) = match result {
        Ok(Ok(ProcessResult::Text(text))) => {
            let trimmed = text.trim();
            let status = if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("no-op")
                || trimmed.eq_ignore_ascii_case("noop")
            {
                "no-op"
            } else {
                "ok"
            };
            (
                status.to_string(),
                json!({"output": text, "source": source}),
            )
        }
        Ok(Ok(_)) => (
            "error".to_string(),
            json!({"error": "Unexpected non-text response", "source": source}),
        ),
        Ok(Err(err)) => (
            "error".to_string(),
            json!({"error": err.to_string(), "source": source}),
        ),
        Err(_) => (
            "error".to_string(),
            json!({"error": "autonomy timeout", "source": source}),
        ),
    };

    let _ = ui_event_tx.send(UiEvent {
        event_type: "autonomy".to_string(),
        user_id,
        tool: "heartbeat".to_string(),
        status,
        payload,
        timestamp: now_ts(),
    });
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn wakeup_audit_log_path(config: Option<&Config>) -> Option<String> {
    let path = config
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| tools.get("wakeup"))
        .and_then(|wakeup| wakeup.get("audit_log_path"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some("./data/wakeup_audit.log".to_string()));
    path
}

fn write_wakeup_audit_log(
    path: Option<&str>,
    ts: i64,
    task: &crate::wakeup::WakeupTask,
    status: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    config_store::ensure_parent_dir(path)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let entry = serde_json::json!({
        "timestamp": ts,
        "task_id": task.id,
        "user_id": task.user_id,
        "name": task.name,
        "prompt": task.prompt,
        "status": status,
        "payload": payload,
    });
    let line = serde_json::to_string(&entry)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}

fn ui_event_log_path(config: Option<&Config>) -> Option<String> {
    config
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| tools.get("settings"))
        .and_then(|settings| settings.get("ui_event_log_path"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some("./data/ui_events.log".to_string()))
}

fn write_ui_event_log(path: &str, event: &UiEvent) -> Result<()> {
    config_store::ensure_parent_dir(path)?;
    let payload = serde_json::to_string(event)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    writeln!(file, "{}", payload).map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}

fn tasks_audit_log_path(config: Option<&Config>) -> Option<String> {
    let path = config
        .and_then(|cfg| cfg.tools.as_ref())
        .and_then(|tools| tools.get("tasks"))
        .and_then(|tasks| tasks.get("audit_log_path"))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| Some("./data/tasks_audit.log".to_string()));
    path
}

fn write_tasks_audit_log(
    path: Option<&str>,
    ts: i64,
    task: &crate::tasks::ScheduledTask,
    status: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    config_store::ensure_parent_dir(path)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let entry = serde_json::json!({
        "timestamp": ts,
        "task_id": task.id,
        "user_id": task.user_id,
        "name": task.name,
        "prompt": task.prompt,
        "run_at": task.run_at,
        "interval_minutes": task.interval_minutes,
        "status": status,
        "payload": payload,
    });
    let line = serde_json::to_string(&entry)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}
