#![allow(
    clippy::clone_on_copy,
    clippy::collapsible_match,
    clippy::collapsible_else_if
)]

use dioxus::document::eval;
use dioxus::launch;
use dioxus::prelude::*;
use futures::StreamExt;
use notify_rust::Notification;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::sync::{Mutex, OnceLock};
use std::thread;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::styled_line_to_highlighted_html;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use time::format_description::well_known::Rfc3339;
use time::{macros::format_description, OffsetDateTime, UtcOffset};
use tokio::time::{sleep, timeout, Duration};

const APP_LOGO: Asset = asset!("/assets/icons/hicolor/32x32/apps/butterfly-bot.png");

#[derive(Clone, Serialize)]
struct ProcessTextRequest {
    user_id: String,
    text: String,
    prompt: Option<String>,
}

#[derive(Clone, Serialize)]
struct PreloadBootRequest {
    user_id: String,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct DoctorCheckResponse {
    name: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
}

#[derive(Clone, Deserialize)]
struct DoctorResponse {
    overall: String,
    checks: Vec<DoctorCheckResponse>,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct SecurityAuditFindingResponse {
    id: String,
    severity: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
    auto_fixable: bool,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct SecurityAuditResponse {
    overall: String,
    findings: Vec<SecurityAuditFindingResponse>,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct FactoryResetConfigResponse {
    message: String,
    config: Value,
}

async fn run_doctor_request(daemon_url: String, token: String) -> Result<DoctorResponse, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/doctor", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("HTTP {status}: {text}"));
    }
    response
        .json::<DoctorResponse>()
        .await
        .map_err(|err| err.to_string())
}

#[allow(dead_code)]
async fn run_security_audit_request(
    daemon_url: String,
    token: String,
) -> Result<SecurityAuditResponse, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/security_audit", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("HTTP {status}: {text}"));
    }
    response
        .json::<SecurityAuditResponse>()
        .await
        .map_err(|err| err.to_string())
}

#[allow(dead_code)]
async fn run_factory_reset_config_request(
    daemon_url: String,
    token: String,
) -> Result<FactoryResetConfigResponse, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/factory_reset_config", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("HTTP {status}: {text}"));
    }
    response
        .json::<FactoryResetConfigResponse>()
        .await
        .map_err(|err| err.to_string())
}

async fn run_chat_history_request(
    daemon_url: String,
    token: String,
    user_id: String,
    limit: usize,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/chat_history?user_id={}&limit={}",
        daemon_url.trim_end_matches('/'),
        user_id,
        limit
    );
    let mut request = client.get(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("HTTP {status}: {text}"));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|err| err.to_string())?;
    let history = value
        .get("history")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(history)
}

async fn run_clear_user_history_request(
    daemon_url: String,
    token: String,
    user_id: String,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/clear_user_history", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request
        .json(&serde_json::json!({"user_id": user_id}))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("HTTP {status}: {text}"));
    }
    Ok(())
}

#[derive(Clone)]
struct ChatMessage {
    id: u64,
    role: MessageRole,
    text: String,
    html: String,
    timestamp: i64,
}

impl ChatMessage {
    fn new(id: u64, role: MessageRole, text: String, timestamp: i64) -> Self {
        let html = markdown_to_html(&text);
        Self {
            id,
            role,
            text,
            html,
            timestamp,
        }
    }

    fn append_text(&mut self, chunk: &str) {
        self.text.push_str(chunk);
        self.html = markdown_to_html(&self.text);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageRole {
    User,
    Bot,
}

const HISTORY_TIMESTAMP_FORMAT: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]");
const MAX_CHAT_MESSAGES: usize = 400;
const MAX_ACTIVITY_MESSAGES: usize = 600;

fn push_bounded_message(list: &mut Vec<ChatMessage>, message: ChatMessage, max_len: usize) {
    list.push(message);
    if list.len() > max_len {
        let overflow = list.len() - max_len;
        list.drain(0..overflow);
    }
}

fn now_unix_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn format_local_time(ts: i64) -> String {
    let dt = OffsetDateTime::from_unix_timestamp(ts)
        .unwrap_or_else(|_| OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(ts.max(0)));
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local = dt.to_offset(local_offset);
    local
        .format(HISTORY_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| ts.to_string())
}

fn parse_history_timestamp(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if let Ok(value) = trimmed.parse::<i64>() {
        return Some(if value >= 1_000_000_000_000 {
            value / 1000
        } else {
            value
        });
    }

    if let Ok(parsed) = OffsetDateTime::parse(trimmed, &Rfc3339) {
        return Some(parsed.unix_timestamp());
    }

    OffsetDateTime::parse(trimmed, HISTORY_TIMESTAMP_FORMAT)
        .ok()
        .map(|value| value.unix_timestamp())
}

fn parse_history_entry(line: &str) -> Option<(MessageRole, String, Option<i64>)> {
    let trimmed = line.trim();
    let payload = trimmed.strip_prefix('[')?;
    let (ts_str, rest) = payload.split_once("] ")?;
    let (role, content) = rest.split_once(": ")?;
    if content.trim().is_empty() {
        return None;
    }
    let role = match role.trim() {
        "user" => MessageRole::User,
        _ => MessageRole::Bot,
    };
    Some((role, content.to_string(), parse_history_timestamp(ts_str)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiTab {
    Chat,
    Activity,
    Config,
    Context,
    Heartbeat,
}

#[derive(Clone, Default)]
struct UiMcpServer {
    name: String,
    url: String,
    header_key: String,
    header_value: String,
}

#[derive(Clone, Default)]
struct UiHttpCallServer {
    name: String,
    url: String,
    header_key: String,
    header_value: String,
}

fn is_url_source(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

async fn load_markdown_source(source: &str) -> Result<String, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if is_url_source(trimmed) {
        let response = reqwest::get(trimmed).await.map_err(|err| err.to_string())?;
        if !response.status().is_success() {
            return Err(format!("Failed to fetch {trimmed}"));
        }
        return response.text().await.map_err(|err| err.to_string());
    }
    Err("Only URL markdown sources are supported for external loading.".to_string())
}

async fn save_markdown_source_to_store(
    db_path: String,
    target: &'static str,
    source: crate::config::MarkdownSource,
) -> Result<(), String> {
    let mut config = crate::config::Config::from_store(&db_path).map_err(|err| err.to_string())?;
    match target {
        "heartbeat" => config.heartbeat_source = source,
        "prompt" => config.prompt_source = source,
        _ => return Err("Unknown markdown source target".to_string()),
    }

    let pretty = serde_json::to_string_pretty(&config).map_err(|err| err.to_string())?;

    let db_path_for_save = db_path.clone();
    let config_for_save = config.clone();
    let save_result = tokio::task::spawn_blocking(move || {
        crate::config_store::save_config(&db_path_for_save, &config_for_save)
    })
    .await
    .map_err(|err| err.to_string())?;

    save_result.map_err(|err| err.to_string())?;
    crate::vault::set_secret("app_config_json", &pretty).map_err(|err| err.to_string())?;
    Ok(())
}

fn markdown_to_html(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(input, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

fn is_empty_list_result(payload: &Value) -> bool {
    let action = payload
        .get("args")
        .and_then(|args| args.get("action"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if action != "list" {
        return false;
    }

    let result = payload.get("result").unwrap_or(&Value::Null);
    ["reminders", "items", "tasks", "plans", "todos", "results"]
        .iter()
        .any(|key| {
            result
                .get(*key)
                .and_then(|value| value.as_array())
                .map(|items| items.is_empty())
                .unwrap_or(false)
        })
}

async fn scroll_chat_to_bottom() {
    let _ = eval("const el = document.getElementById('chat-scroll'); if (el) { el.scrollTop = el.scrollHeight; }").await;
}

async fn scroll_chat_after_render() {
    scroll_chat_to_bottom().await;
    sleep(Duration::from_millis(16)).await;
    scroll_chat_to_bottom().await;
}

async fn scroll_activity_to_bottom() {
    let _ = eval("const el = document.getElementById('activity-scroll'); if (el) { el.scrollTop = el.scrollHeight; }").await;
}

async fn scroll_activity_after_render() {
    scroll_activity_to_bottom().await;
    sleep(Duration::from_millis(16)).await;
    scroll_activity_to_bottom().await;
}

#[allow(dead_code)]
fn highlight_json_html(input: &str) -> String {
    static SYNTAX_SET: once_cell::sync::Lazy<SyntaxSet> =
        once_cell::sync::Lazy::new(SyntaxSet::load_defaults_newlines);
    static THEMES: once_cell::sync::Lazy<ThemeSet> =
        once_cell::sync::Lazy::new(ThemeSet::load_defaults);

    let syntax = SYNTAX_SET
        .find_syntax_by_extension("json")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = THEMES
        .themes
        .get("base16-ocean.dark")
        .or_else(|| THEMES.themes.values().next())
        .expect("theme available");

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut out = String::new();
    for line in LinesWithEndings::from(input) {
        let ranges = highlighter
            .highlight_line(line, &SYNTAX_SET)
            .unwrap_or_default();
        let html =
            styled_line_to_highlighted_html(&ranges[..], syntect::html::IncludeBackground::No)
                .unwrap_or_default();
        out.push_str(&html);
    }
    out
}

pub fn launch_ui() {
    force_dbusrs();
    launch(app_view);
}

fn stream_timeout_duration() -> Duration {
    let default_secs = 180u64;
    let value = std::env::var("BUTTERFLY_BOT_STREAM_TIMEOUT_SECONDS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0);
    Duration::from_secs(value.unwrap_or(default_secs))
}

#[cfg(target_os = "linux")]
fn force_dbusrs() {
    if std::env::var("DBUSRS").is_err() {
        std::env::set_var("DBUSRS", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn force_dbusrs() {}

struct DaemonControl {
    shutdown: tokio::sync::oneshot::Sender<()>,
    thread: thread::JoinHandle<()>,
}

fn daemon_control() -> &'static Mutex<Option<DaemonControl>> {
    static CONTROL: OnceLock<Mutex<Option<DaemonControl>>> = OnceLock::new();
    CONTROL.get_or_init(|| Mutex::new(None))
}

fn start_local_daemon() -> Result<(), String> {
    if env::var("BUTTERFLY_BOT_DISABLE_DAEMON").is_ok() {
        return Err("Daemon disabled by BUTTERFLY_BOT_DISABLE_DAEMON".to_string());
    }

    let control = daemon_control();
    let mut guard = control
        .lock()
        .map_err(|_| "Daemon lock unavailable".to_string())?;
    if guard.is_some() {
        return Ok(());
    }

    let daemon_url =
        env::var("BUTTERFLY_BOT_DAEMON").unwrap_or_else(|_| "http://127.0.0.1:7878".to_string());
    let (host, port) = parse_daemon_address(&daemon_url);
    let db_path =
        env::var("BUTTERFLY_BOT_DB").unwrap_or_else(|_| crate::runtime_paths::default_db_path());
    let token = env_auth_token();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let thread = thread::spawn(move || {
        if let Ok(runtime) = tokio::runtime::Runtime::new() {
            runtime.block_on(async move {
                let shutdown = async move {
                    let _ = shutdown_rx.await;
                };
                let _ =
                    crate::daemon::run_with_shutdown(&host, port, &db_path, &token, shutdown).await;
            });
        }
    });

    *guard = Some(DaemonControl {
        shutdown: shutdown_tx,
        thread,
    });

    Ok(())
}

fn env_auth_token() -> String {
    crate::vault::ensure_daemon_auth_token().unwrap_or_default()
}

fn stop_local_daemon() -> Result<(), String> {
    let control = daemon_control();
    let mut guard = control
        .lock()
        .map_err(|_| "Daemon lock unavailable".to_string())?;
    if let Some(control) = guard.take() {
        let _ = control.shutdown.send(());
        thread::spawn(move || {
            let _ = control.thread.join();
        });
        Ok(())
    } else {
        Err("Daemon is not running".to_string())
    }
}

fn normalize_daemon_url(daemon: &str) -> String {
    let trimmed = daemon.trim();
    let (scheme, rest) = if let Some(value) = trimmed.strip_prefix("https://") {
        ("https://", value)
    } else if let Some(value) = trimmed.strip_prefix("http://") {
        ("http://", value)
    } else {
        ("http://", trimmed)
    };
    let host_port = rest.split('/').next().unwrap_or("127.0.0.1:7878");
    format!("{scheme}{host_port}")
}

fn parse_daemon_address(daemon: &str) -> (String, u16) {
    let trimmed = daemon.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let host_port = without_scheme.split('/').next().unwrap_or("127.0.0.1:7878");
    let mut parts = host_port.splitn(2, ':');
    let host = parts.next().unwrap_or("127.0.0.1");
    let port = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(7878);
    (host.to_string(), port)
}

fn app_view() -> Element {
    let db_path =
        env::var("BUTTERFLY_BOT_DB").unwrap_or_else(|_| crate::runtime_paths::default_db_path());
    let daemon_url = use_signal(|| {
        let raw = env::var("BUTTERFLY_BOT_DAEMON")
            .unwrap_or_else(|_| "http://127.0.0.1:7878".to_string());
        normalize_daemon_url(&raw)
    });
    let token = use_signal(env_auth_token);
    let user_id =
        use_signal(|| env::var("BUTTERFLY_BOT_USER_ID").unwrap_or_else(|_| "user".to_string()));
    let input = use_signal(String::new);
    let busy = use_signal(|| false);
    let error = use_signal(String::new);
    let messages = use_signal(Vec::<ChatMessage>::new);
    let activity_messages = use_signal(Vec::<ChatMessage>::new);
    let daemon_running = use_signal(|| false);
    let daemon_autostart_attempted = use_signal(|| false);
    let daemon_status = use_signal(String::new);
    let next_id = use_signal(|| 1u64);
    let active_tab = use_signal(|| UiTab::Chat);
    let reminders_listening = use_signal(|| false);
    let reminders_listener_started = use_signal(|| false);
    let ui_events_listening = use_signal(|| false);
    let ui_events_listener_started = use_signal(|| false);
    let history_load_started = use_signal(|| false);
    let last_bot_scroll_id = use_signal(|| 0u64);

    let tools_loaded = use_signal(|| false);
    let settings_load_started = use_signal(|| false);
    let boot_ready = use_signal(|| false);
    let boot_status = use_signal(String::new);
    let boot_prompt_ready = use_signal(|| false);
    let boot_heartbeat_ready = use_signal(|| false);
    let settings_error = use_signal(String::new);
    let settings_status = use_signal(String::new);
    let doctor_status = use_signal(String::new);
    let doctor_error = use_signal(String::new);
    let doctor_running = use_signal(|| false);
    let doctor_overall = use_signal(String::new);
    let doctor_checks = use_signal(Vec::<DoctorCheckResponse>::new);
    let security_audit_status = use_signal(String::new);
    let security_audit_error = use_signal(String::new);
    let _security_audit_running = use_signal(|| false);
    let security_audit_overall = use_signal(String::new);
    let security_audit_findings = use_signal(Vec::<SecurityAuditFindingResponse>::new);
    let config_json_text = use_signal(String::new);
    let wakeup_poll_seconds_input = use_signal(|| "60".to_string());
    let github_pat_input = use_signal(String::new);
    let zapier_token_input = use_signal(String::new);
    let coding_api_key_input = use_signal(String::new);
    let search_api_key_input = use_signal(String::new);
    let mcp_servers_form = use_signal(Vec::<UiMcpServer>::new);
    let http_call_servers_form = use_signal(Vec::<UiHttpCallServer>::new);
    let network_allow_form = use_signal(Vec::<String>::new);
    let context_text = use_signal(String::new);
    let context_path = use_signal(|| "database".to_string());
    let context_status = use_signal(String::new);
    let context_error = use_signal(String::new);
    let heartbeat_text = use_signal(String::new);
    let heartbeat_path = use_signal(|| "database".to_string());
    let heartbeat_status = use_signal(String::new);
    let heartbeat_error = use_signal(String::new);

    let search_provider = use_signal(|| "openai".to_string());
    let search_model = use_signal(String::new);
    let search_citations = use_signal(|| true);
    let search_grok_web = use_signal(|| true);
    let search_grok_x = use_signal(|| true);
    let search_grok_timeout = use_signal(|| "90".to_string());
    let search_network_allow = use_signal(String::new);
    let search_default_deny = use_signal(|| false);
    let search_api_key_status = use_signal(String::new);

    let reminders_sqlite_path = use_signal(String::new);
    let memory_enabled = use_signal(|| true);

    let on_send = {
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let input = input.clone();
        let busy = busy.clone();
        let error = error.clone();
        let messages = messages.clone();
        let next_id = next_id.clone();

        use_callback(move |_: ()| {
            let daemon_url = daemon_url();
            let token = token();
            let user_id = user_id();
            let text = input();
            let busy = busy.clone();
            let error = error.clone();
            let messages = messages.clone();
            let next_id = next_id.clone();

            spawn(async move {
                let mut busy = busy;
                let mut error = error;
                let mut messages = messages;
                let mut next_id = next_id;
                let mut input = input;

                if *busy.read() {
                    error.set("A request is already in progress. Please wait.".to_string());
                    return;
                }

                if text.trim().is_empty() {
                    error.set("Enter a message to send.".to_string());
                    return;
                }

                busy.set(true);
                error.set(String::new());

                let user_message_id = {
                    let id = next_id();
                    next_id.set(id + 1);
                    id
                };
                let bot_message_id = {
                    let id = next_id();
                    next_id.set(id + 1);
                    id
                };
                let timestamp = now_unix_ts();

                {
                    let mut list = messages.write();
                    push_bounded_message(
                        &mut list,
                        ChatMessage::new(
                            user_message_id,
                            MessageRole::User,
                            text.clone(),
                            timestamp,
                        ),
                        MAX_CHAT_MESSAGES,
                    );
                    push_bounded_message(
                        &mut list,
                        ChatMessage::new(
                            bot_message_id,
                            MessageRole::Bot,
                            String::new(),
                            timestamp,
                        ),
                        MAX_CHAT_MESSAGES,
                    );
                }

                input.set(String::new());
                scroll_chat_after_render().await;

                let client = reqwest::Client::new();
                let url = format!("{}/process_text_stream", daemon_url.trim_end_matches('/'));
                let body = ProcessTextRequest {
                    user_id,
                    text,
                    prompt: None,
                };

                let make_request = |client: &reqwest::Client,
                                    url: &str,
                                    token: &str,
                                    body: &ProcessTextRequest| {
                    let mut request = client.post(url);
                    if !token.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token}"));
                    }
                    request.json(body)
                };

                match make_request(&client, &url, &token, &body).send().await {
                    Ok(response) => {
                        let mut messages = messages.clone();
                        let mut error = error.clone();
                        if response.status().is_success() {
                            let mut stream = response.bytes_stream();
                            let mut chunk_counter = 0usize;
                            loop {
                                let next_chunk =
                                    match timeout(stream_timeout_duration(), stream.next()).await {
                                        Ok(value) => value,
                                        Err(_) => {
                                            error.set(
                                                "Stream timed out waiting for response."
                                                    .to_string(),
                                            );
                                            break;
                                        }
                                    };
                                let Some(chunk) = next_chunk else {
                                    break;
                                };
                                match chunk {
                                    Ok(bytes) => {
                                        if let Ok(text_chunk) = std::str::from_utf8(&bytes) {
                                            if !text_chunk.is_empty() {
                                                let mut list = messages.write();
                                                if let Some(last) = list
                                                    .iter_mut()
                                                    .rev()
                                                    .find(|msg| msg.id == bot_message_id)
                                                {
                                                    last.append_text(text_chunk);
                                                }
                                                chunk_counter += 1;
                                            }
                                        }
                                        if chunk_counter > 0 && chunk_counter.is_multiple_of(8) {
                                            scroll_chat_to_bottom().await;
                                        }
                                    }
                                    Err(err) => {
                                        error.set(format!("Stream error: {err}"));
                                        break;
                                    }
                                }
                            }
                            if chunk_counter > 0 {
                                scroll_chat_after_render().await;
                            }
                        } else {
                            let status = response.status();
                            let text = response
                                .text()
                                .await
                                .unwrap_or_else(|_| "Unable to read error body".to_string());
                            error.set(format!("Request failed ({status}): {text}"));
                        }
                    }
                    Err(err) => {
                        error.set(format!(
                            "Request failed: {err}. Daemon unreachable at {daemon_url}. Use Start on the main page (Chat tab)."
                        ));
                    }
                }

                busy.set(false);
            });
        })
    };
    let on_send_key = on_send.clone();

    let on_clear_histories = {
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let messages = messages.clone();
        let activity_messages = activity_messages.clone();
        let next_id = next_id.clone();
        let error = error.clone();

        use_callback(move |_: ()| {
            let daemon_running = daemon_running.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();
            let user_id = user_id.clone();
            let messages = messages.clone();
            let activity_messages = activity_messages.clone();
            let next_id = next_id.clone();
            let error = error.clone();

            spawn(async move {
                let mut messages = messages;
                let mut activity_messages = activity_messages;
                let mut next_id = next_id;
                let mut error = error;

                messages.set(Vec::new());
                activity_messages.set(Vec::new());
                next_id.set(1);
                error.set(String::new());

                if daemon_running() {
                    if let Err(err) =
                        run_clear_user_history_request(daemon_url(), token(), user_id()).await
                    {
                        error.set(format!(
                            "Cleared local history, but daemon clear failed: {err}"
                        ));
                    }
                }
            });
        })
    };

    let on_daemon_start = {
        let daemon_status = daemon_status.clone();
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let boot_ready = boot_ready.clone();
        let boot_status = boot_status.clone();
        let doctor_status = doctor_status.clone();
        let doctor_error = doctor_error.clone();
        let doctor_running = doctor_running.clone();
        let doctor_overall = doctor_overall.clone();
        let doctor_checks = doctor_checks.clone();

        use_callback(move |_: ()| {
            let daemon_status = daemon_status.clone();
            let daemon_running = daemon_running.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();
            let user_id = user_id.clone();
            let boot_ready = boot_ready.clone();
            let boot_status = boot_status.clone();
            let doctor_status = doctor_status.clone();
            let doctor_error = doctor_error.clone();
            let doctor_running = doctor_running.clone();
            let doctor_overall = doctor_overall.clone();
            let doctor_checks = doctor_checks.clone();
            spawn(async move {
                let mut daemon_status = daemon_status;
                let mut daemon_running = daemon_running;
                let mut boot_ready = boot_ready;
                let mut boot_status = boot_status;
                let mut doctor_status = doctor_status;
                let mut doctor_error = doctor_error;
                let mut doctor_running = doctor_running;
                let mut doctor_overall = doctor_overall;
                let mut doctor_checks = doctor_checks;
                let result = start_local_daemon();
                match result {
                    Ok(()) => {
                        daemon_status.set("Daemon started.".to_string());
                        boot_ready.set(true);
                        boot_status.set(
                            "Daemon starting… boot preload will continue in background."
                                .to_string(),
                        );

                        // Wait for daemon to be ready (retry up to 10 times with 500ms delay)
                        let client = reqwest::Client::new();
                        let mut daemon_ready = false;
                        for i in 0..10 {
                            sleep(Duration::from_millis(500)).await;
                            let health_url =
                                format!("{}/health", daemon_url().trim_end_matches('/'));
                            if let Ok(resp) = client.get(&health_url).send().await {
                                if resp.status().is_success() {
                                    daemon_ready = true;
                                    break;
                                }
                            }
                            boot_status.set(format!("Waiting for daemon... ({}/10)", i + 1));
                        }

                        if !daemon_ready {
                            daemon_running.set(false);
                            boot_status.set(
                                "Daemon started but not responding. Continuing without preload."
                                    .to_string(),
                            );
                            boot_ready.set(true);
                        } else {
                            daemon_running.set(true);
                            let url =
                                format!("{}/preload_boot", daemon_url().trim_end_matches('/'));
                            let mut request = client
                                .post(&url)
                                .json(&PreloadBootRequest { user_id: user_id() });
                            let token_value = token();
                            if !token_value.trim().is_empty() {
                                request = request
                                    .header("authorization", format!("Bearer {token_value}"));
                            }
                            match request.send().await {
                                Ok(resp) if resp.status().is_success() => {
                                    boot_status
                                        .set("Boot preload started in background…".to_string());
                                }
                                Ok(resp) => {
                                    let status = resp.status();
                                    boot_status.set(format!("Boot preload failed: HTTP {status}"));
                                }
                                Err(err) => {
                                    boot_status.set(format!("Boot preload error: {err}"));
                                }
                            }

                            doctor_running.set(true);
                            doctor_error.set(String::new());
                            doctor_status.set("Running diagnostics…".to_string());
                            match run_doctor_request(daemon_url(), token()).await {
                                Ok(report) => {
                                    let overall = report.overall.clone();
                                    doctor_overall.set(overall.clone());
                                    doctor_checks.set(report.checks);
                                    doctor_status.set(format!("Diagnostics complete ({overall})."));
                                }
                                Err(err) => {
                                    doctor_error.set(format!("Diagnostics failed: {err}"));
                                    doctor_status.set(String::new());
                                }
                            }
                            doctor_running.set(false);
                        }
                    }
                    Err(err) => {
                        daemon_status.set(err);
                    }
                }
            });
        })
    };

    let on_daemon_stop = {
        let daemon_status = daemon_status.clone();
        let daemon_running = daemon_running.clone();
        let reminders_listening = reminders_listening.clone();
        let ui_events_listening = ui_events_listening.clone();
        let boot_ready = boot_ready.clone();
        let boot_status = boot_status.clone();
        let doctor_status = doctor_status.clone();
        let doctor_error = doctor_error.clone();
        let doctor_overall = doctor_overall.clone();
        let doctor_checks = doctor_checks.clone();
        let security_audit_status = security_audit_status.clone();
        let security_audit_error = security_audit_error.clone();
        let security_audit_overall = security_audit_overall.clone();
        let security_audit_findings = security_audit_findings.clone();

        use_callback(move |_: ()| {
            let daemon_status = daemon_status.clone();
            let daemon_running = daemon_running.clone();
            let reminders_listening = reminders_listening.clone();
            let ui_events_listening = ui_events_listening.clone();
            let boot_ready = boot_ready.clone();
            let boot_status = boot_status.clone();
            let doctor_status = doctor_status.clone();
            let doctor_error = doctor_error.clone();
            let doctor_overall = doctor_overall.clone();
            let doctor_checks = doctor_checks.clone();
            let security_audit_status = security_audit_status.clone();
            let security_audit_error = security_audit_error.clone();
            let security_audit_overall = security_audit_overall.clone();
            let security_audit_findings = security_audit_findings.clone();
            spawn(async move {
                let mut daemon_status = daemon_status;
                let mut daemon_running = daemon_running;
                let mut reminders_listening = reminders_listening;
                let mut ui_events_listening = ui_events_listening;
                let mut boot_ready = boot_ready;
                let mut boot_status = boot_status;
                let mut doctor_status = doctor_status;
                let mut doctor_error = doctor_error;
                let mut doctor_overall = doctor_overall;
                let mut doctor_checks = doctor_checks;
                let mut security_audit_status = security_audit_status;
                let mut security_audit_error = security_audit_error;
                let mut security_audit_overall = security_audit_overall;
                let mut security_audit_findings = security_audit_findings;
                let result = stop_local_daemon();
                match result {
                    Ok(()) => {
                        daemon_running.set(false);
                        reminders_listening.set(false);
                        ui_events_listening.set(false);
                        boot_ready.set(false);
                        boot_status.set(
                            "Daemon stopped. Start it to preload prompt + heartbeat.".to_string(),
                        );
                        daemon_status.set("Daemon stopped.".to_string());
                        doctor_status.set(String::new());
                        doctor_error.set(String::new());
                        doctor_overall.set(String::new());
                        doctor_checks.set(Vec::new());
                        security_audit_status.set(String::new());
                        security_audit_error.set(String::new());
                        security_audit_overall.set(String::new());
                        security_audit_findings.set(Vec::new());
                    }
                    Err(err) => {
                        daemon_status.set(err);
                    }
                }
            });
        })
    };

    {
        let daemon_autostart_attempted = daemon_autostart_attempted.clone();
        let daemon_running = daemon_running.clone();
        let daemon_status = daemon_status.clone();
        let on_daemon_start = on_daemon_start.clone();

        use_effect(move || {
            if *daemon_autostart_attempted.read() {
                return;
            }
            let mut daemon_autostart_attempted = daemon_autostart_attempted.clone();
            daemon_autostart_attempted.set(true);

            if *daemon_running.read() {
                return;
            }

            let mut daemon_status = daemon_status.clone();
            daemon_status.set("Starting daemon for zero-step onboarding…".to_string());
            on_daemon_start.call(());
        });
    }

    {
        let messages = messages.clone();
        let last_bot_scroll_id = last_bot_scroll_id.clone();

        use_effect(move || {
            let latest_bot_id = messages
                .read()
                .iter()
                .rev()
                .find(|msg| msg.role == MessageRole::Bot && !msg.text.is_empty())
                .map(|msg| msg.id)
                .unwrap_or(0);

            if latest_bot_id == 0 || latest_bot_id == *last_bot_scroll_id.read() {
                return;
            }

            let mut last_bot_scroll_id = last_bot_scroll_id.clone();
            last_bot_scroll_id.set(latest_bot_id);

            spawn(async move {
                scroll_chat_after_render().await;
            });
        });
    }

    {
        let history_load_started = history_load_started.clone();
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let messages = messages.clone();
        let activity_messages = activity_messages.clone();
        let activity_messages = activity_messages.clone();
        let next_id = next_id.clone();

        use_effect(move || {
            if *history_load_started.read() || !*daemon_running.read() {
                return;
            }

            let mut started = history_load_started.clone();
            started.set(true);

            let mut messages = messages.clone();
            let mut activity_messages = activity_messages.clone();
            let mut next_id = next_id.clone();

            spawn(async move {
                let history =
                    match run_chat_history_request(daemon_url(), token(), user_id(), 40).await {
                        Ok(history) => history,
                        Err(_) => return,
                    };

                if history.is_empty() || !messages.read().is_empty() {
                    return;
                }

                let parsed = history
                    .into_iter()
                    .filter_map(|line| parse_history_entry(&line))
                    .collect::<Vec<_>>();

                if parsed.is_empty() || !messages.read().is_empty() {
                    return;
                }

                let mut list = messages.write();
                if !list.is_empty() {
                    return;
                }

                for (role, text, timestamp) in parsed {
                    let id = next_id();
                    next_id.set(id + 1);
                    push_bounded_message(
                        &mut list,
                        ChatMessage::new(id, role, text, timestamp.unwrap_or_else(now_unix_ts)),
                        MAX_CHAT_MESSAGES,
                    );
                }

                let mut activity = activity_messages.write();
                if activity.is_empty() {
                    for entry in list.iter().cloned() {
                        push_bounded_message(&mut activity, entry, MAX_ACTIVITY_MESSAGES);
                    }
                }

                drop(list);
                scroll_chat_after_render().await;
            });
        });
    }

    {
        let reminders_listener_started = reminders_listener_started.clone();
        let reminders_listening = reminders_listening.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let daemon_running = daemon_running.clone();
        let messages = messages.clone();
        let next_id = next_id.clone();

        use_effect(move || {
            if *reminders_listener_started.read() {
                return;
            }
            let mut started = reminders_listener_started.clone();
            started.set(true);

            spawn(async move {
                let mut reminders_listening = reminders_listening;
                let daemon_url = daemon_url;
                let token = token;
                let user_id = user_id;
                let mut messages = messages;
                let mut next_id = next_id;

                reminders_listening.set(true);
                let client = reqwest::Client::new();
                loop {
                    if !*daemon_running.read() {
                        reminders_listening.set(false);
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    reminders_listening.set(true);
                    let url = format!(
                        "{}/reminder_stream?user_id={}",
                        daemon_url().trim_end_matches('/'),
                        user_id()
                    );
                    let mut request = client.get(&url);
                    let token_value = token();
                    if !token_value.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token_value}"));
                    }

                    let response = match request.send().await {
                        Ok(resp) => resp,
                        Err(_) => {
                            if std::env::var("BUTTERFLY_BOT_REMINDER_DEBUG").is_ok()
                                || cfg!(debug_assertions)
                            {
                                eprintln!("Reminder stream request failed (daemon unreachable?)");
                            }
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    if !response.status().is_success() {
                        if std::env::var("BUTTERFLY_BOT_REMINDER_DEBUG").is_ok()
                            || cfg!(debug_assertions)
                        {
                            eprintln!("Reminder stream error: HTTP {}", response.status());
                        }
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    while let Some(chunk) = stream.next().await {
                        let Ok(chunk) = chunk else {
                            break;
                        };
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            buffer.push_str(text);
                            while let Some(idx) = buffer.find('\n') {
                                let mut line = buffer[..idx].to_string();
                                buffer = buffer[idx + 1..].to_string();
                                if line.starts_with("data:") {
                                    line = line.trim_start_matches("data:").trim().to_string();
                                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                                        let title = value
                                            .get("title")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Reminder");
                                        let id = next_id();
                                        next_id.set(id + 1);
                                        let timestamp = value
                                            .get("due_at")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or_else(now_unix_ts);
                                        push_bounded_message(
                                            &mut messages.write(),
                                            ChatMessage::new(
                                                id,
                                                MessageRole::Bot,
                                                format!("⏰ {title}"),
                                                timestamp,
                                            ),
                                            MAX_CHAT_MESSAGES,
                                        );
                                        scroll_chat_to_bottom().await;
                                        if let Err(err) = Notification::new()
                                            .summary("Butterfly Bot")
                                            .body(title)
                                            .show()
                                        {
                                            eprintln!("Notification error: {err}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    sleep(Duration::from_secs(2)).await;
                }
            });
        });
    }

    {
        let active_tab = active_tab.clone();
        use_effect(move || {
            let tab = *active_tab.read();
            spawn(async move {
                match tab {
                    UiTab::Chat => scroll_chat_after_render().await,
                    UiTab::Activity => scroll_activity_after_render().await,
                    _ => {}
                }
            });
        });
    }

    {
        let ui_events_listener_started = ui_events_listener_started.clone();
        let ui_events_listening = ui_events_listening.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let activity_messages = activity_messages.clone();
        let next_id = next_id.clone();
        let daemon_running = daemon_running.clone();
        let mut boot_ready = boot_ready.clone();
        let mut boot_status = boot_status.clone();
        let mut boot_prompt_ready = boot_prompt_ready.clone();
        let mut boot_heartbeat_ready = boot_heartbeat_ready.clone();

        use_effect(move || {
            if *ui_events_listener_started.read() {
                return;
            }
            let mut started = ui_events_listener_started.clone();
            started.set(true);

            spawn(async move {
                let mut ui_events_listening = ui_events_listening;
                let daemon_url = daemon_url;
                let token = token;
                let user_id = user_id;
                let mut activity_messages = activity_messages;
                let mut next_id = next_id;

                ui_events_listening.set(true);
                let client = reqwest::Client::new();
                loop {
                    if !*daemon_running.read() {
                        ui_events_listening.set(false);
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    ui_events_listening.set(true);
                    let url = format!(
                        "{}/ui_events?user_id={}",
                        daemon_url().trim_end_matches('/'),
                        user_id()
                    );
                    let mut request = client.get(&url);
                    let token_value = token();
                    if !token_value.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token_value}"));
                    }

                    let response = match request.send().await {
                        Ok(resp) => resp,
                        Err(_) => {
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    if !response.status().is_success() {
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    while let Some(chunk) = stream.next().await {
                        let Ok(chunk) = chunk else {
                            break;
                        };
                        if let Ok(text) = std::str::from_utf8(&chunk) {
                            buffer.push_str(text);
                            while let Some(idx) = buffer.find('\n') {
                                let mut line = buffer[..idx].to_string();
                                buffer = buffer[idx + 1..].to_string();
                                if line.starts_with("data:") {
                                    line = line.trim_start_matches("data:").trim().to_string();
                                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                                        let event_type = value
                                            .get("event_type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("tool");
                                        let tool = value
                                            .get("tool")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("tool");
                                        let status = value
                                            .get("status")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("ok");
                                        let event_user = value
                                            .get("user_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default();

                                        // Always update boot readiness for boot/prompt/heartbeat events
                                        if (event_type == "boot" || tool == "prompt")
                                            && status == "ok"
                                        {
                                            boot_prompt_ready.set(true);
                                        }
                                        if (event_type == "boot" || tool == "heartbeat")
                                            && status == "ok"
                                        {
                                            boot_heartbeat_ready.set(true);
                                        }
                                        if *boot_prompt_ready.read() && *boot_heartbeat_ready.read()
                                        {
                                            boot_ready.set(true);
                                            boot_status.set("Prompt + heartbeat ready".to_string());
                                        }

                                        let show_success =
                                            std::env::var("BUTTERFLY_BOT_SHOW_TOOL_SUCCESS")
                                                .is_ok();
                                        if !show_success && (status == "success" || status == "ok")
                                        {
                                            if event_type == "tool" {
                                                if event_user == "system" {
                                                    continue;
                                                }
                                                if let Some(payload) = value.get("payload") {
                                                    if payload.get("error").is_none()
                                                        && is_empty_list_result(payload)
                                                    {
                                                        continue;
                                                    }
                                                }
                                            } else if event_type != "boot"
                                                && event_type != "autonomy"
                                            {
                                                if let Some(payload) = value.get("payload") {
                                                    if payload.get("error").is_none() {
                                                        continue;
                                                    }
                                                } else {
                                                    continue;
                                                }
                                            }
                                        }

                                        let prefix = if event_type == "autonomy" {
                                            "🤖"
                                        } else {
                                            "🔧"
                                        };
                                        let mut text = format!("{prefix} {tool}: {status}");
                                        if let Some(payload) = value.get("payload") {
                                            if let Some(error) =
                                                payload.get("error").and_then(|v| v.as_str())
                                            {
                                                text.push_str(&format!(" — {error}"));
                                            } else if event_type == "autonomy" {
                                                if status == "skipped" {
                                                    if let Some(reason) = payload
                                                        .get("reason")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        text.push_str(&format!(" — {reason}"));
                                                    }
                                                    if let Some(remaining) = payload
                                                        .get("cooldown_remaining_seconds")
                                                        .and_then(|v| v.as_i64())
                                                    {
                                                        text.push_str(&format!(
                                                            " ({}s)",
                                                            remaining.max(0)
                                                        ));
                                                    }
                                                }
                                            } else if let Some(output) = payload
                                                .get("output")
                                                .or_else(|| payload.get("response"))
                                                .and_then(|v| v.as_str())
                                            {
                                                text.push_str(&format!(" — {output}"));
                                            }
                                        }
                                        let id = next_id();
                                        next_id.set(id + 1);
                                        let timestamp = value
                                            .get("timestamp")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or_else(now_unix_ts);
                                        push_bounded_message(
                                            &mut activity_messages.write(),
                                            ChatMessage::new(id, MessageRole::Bot, text, timestamp),
                                            MAX_ACTIVITY_MESSAGES,
                                        );
                                        scroll_activity_after_render().await;
                                    }
                                }
                            }
                        }
                    }
                    sleep(Duration::from_secs(2)).await;
                }
            });
        });
    }

    {
        let settings_load_started = settings_load_started.clone();
        let settings_error = settings_error.clone();
        let tools_loaded = tools_loaded.clone();
        let settings_status = settings_status.clone();
        let config_json_text = config_json_text.clone();
        let wakeup_poll_seconds_input = wakeup_poll_seconds_input.clone();
        let github_pat_input = github_pat_input.clone();
        let zapier_token_input = zapier_token_input.clone();
        let coding_api_key_input = coding_api_key_input.clone();
        let search_api_key_input = search_api_key_input.clone();
        let mcp_servers_form = mcp_servers_form.clone();
        let http_call_servers_form = http_call_servers_form.clone();
        let network_allow_form = network_allow_form.clone();
        let boot_status = boot_status.clone();
        let boot_ready = boot_ready.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let search_provider = search_provider.clone();
        let search_model = search_model.clone();
        let search_citations = search_citations.clone();
        let search_grok_web = search_grok_web.clone();
        let search_grok_x = search_grok_x.clone();
        let search_grok_timeout = search_grok_timeout.clone();
        let search_network_allow = search_network_allow.clone();
        let search_default_deny = search_default_deny.clone();
        let search_api_key_status = search_api_key_status.clone();
        let reminders_sqlite_path = reminders_sqlite_path.clone();
        let memory_enabled = memory_enabled.clone();
        let context_text = context_text.clone();
        let context_path = context_path.clone();
        let context_error = context_error.clone();
        let heartbeat_text = heartbeat_text.clone();
        let heartbeat_path = heartbeat_path.clone();
        let heartbeat_error = heartbeat_error.clone();
        let db_path = db_path.clone();

        use_effect(move || {
            if *settings_load_started.read() {
                return;
            }
            let mut started = settings_load_started.clone();
            started.set(true);
            let db_path = db_path.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut tools_loaded = tools_loaded;
                let mut settings_status = settings_status;
                let mut config_json_text = config_json_text;
                let mut wakeup_poll_seconds_input = wakeup_poll_seconds_input;
                let mut github_pat_input = github_pat_input;
                let mut zapier_token_input = zapier_token_input;
                let mut coding_api_key_input = coding_api_key_input;
                let mut search_api_key_input = search_api_key_input;
                let mut mcp_servers_form = mcp_servers_form;
                let mut http_call_servers_form = http_call_servers_form;
                let mut network_allow_form = network_allow_form;
                let mut search_provider = search_provider;
                let mut search_model = search_model;
                let mut search_citations = search_citations;
                let mut search_grok_web = search_grok_web;
                let mut search_grok_x = search_grok_x;
                let mut search_grok_timeout = search_grok_timeout;
                let mut search_network_allow = search_network_allow;
                let mut search_default_deny = search_default_deny;
                let mut search_api_key_status = search_api_key_status;
                let mut reminders_sqlite_path = reminders_sqlite_path;
                let mut memory_enabled = memory_enabled;
                let mut context_text = context_text;
                let mut context_path = context_path;
                let mut context_error = context_error;
                let mut heartbeat_text = heartbeat_text;
                let mut heartbeat_path = heartbeat_path;
                let heartbeat_error = heartbeat_error;
                let mut boot_status = boot_status;
                let mut boot_ready = boot_ready;

                let config = match crate::config::Config::from_store(&db_path) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Failed to load config: {err}"));
                        tools_loaded.set(true);
                        return;
                    }
                };

                match crate::vault::get_secret("app_config_json") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        config_json_text.set(secret);
                        settings_status.set("Loaded config from keyring.".to_string());
                    }
                    Ok(_) => {
                        if let Ok(pretty) = serde_json::to_string_pretty(&config) {
                            config_json_text.set(pretty);
                        }
                    }
                    Err(err) => {
                        settings_error.set(format!("Vault error: {err}"));
                    }
                }

                let mut allowlist: Vec<String> = Vec::new();
                let mut default_deny = false;

                if let Some(tools_value) = &config.tools {
                    if let Value::Object(map) = tools_value {
                        if let Some(settings) = map.get("settings").and_then(|v| v.as_object()) {
                            if let Some(perms) = settings.get("permissions") {
                                if let Some(items) =
                                    perms.get("network_allow").and_then(|v| v.as_array())
                                {
                                    allowlist = items
                                        .iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect();
                                }
                                if let Some(value) =
                                    perms.get("default_deny").and_then(|v| v.as_bool())
                                {
                                    default_deny = value;
                                }
                            }
                        }
                    }
                }

                let enabled = config
                    .memory
                    .as_ref()
                    .and_then(|memory| memory.enabled)
                    .unwrap_or(true);
                memory_enabled.set(enabled);

                if let Some(tools_value) = &config.tools {
                    if let Some(wakeup_cfg) = tools_value.get("wakeup") {
                        if let Some(poll_seconds) =
                            wakeup_cfg.get("poll_seconds").and_then(|v| v.as_u64())
                        {
                            wakeup_poll_seconds_input.set(poll_seconds.to_string());
                        }
                    }

                    if let Some(mcp_servers) = tools_value
                        .get("mcp")
                        .and_then(|mcp| mcp.get("servers"))
                        .and_then(|servers| servers.as_array())
                    {
                        let parsed_servers = mcp_servers
                            .iter()
                            .filter_map(|entry| {
                                let name = entry
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                let url = entry
                                    .get("url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                let (header_key, header_value) = entry
                                    .get("headers")
                                    .and_then(|v| v.as_object())
                                    .and_then(|map| {
                                        map.iter().find_map(|(k, v)| {
                                            v.as_str().map(|value| {
                                                (k.trim().to_string(), value.trim().to_string())
                                            })
                                        })
                                    })
                                    .unwrap_or_else(|| (String::new(), String::new()));
                                if name.is_empty() && url.is_empty() {
                                    None
                                } else {
                                    Some(UiMcpServer {
                                        name,
                                        url,
                                        header_key,
                                        header_value,
                                    })
                                }
                            })
                            .collect::<Vec<_>>();
                        mcp_servers_form.set(parsed_servers);
                    }

                    if let Some(http_call_cfg) = tools_value.get("http_call") {
                        let mut parsed_servers = http_call_cfg
                            .get("servers")
                            .and_then(|v| v.as_array())
                            .map(|servers| {
                                servers
                                    .iter()
                                    .filter_map(|entry| {
                                        let name = entry
                                            .get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        let url = entry
                                            .get("url")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        let (header_key, header_value) = entry
                                            .get("headers")
                                            .and_then(|v| v.as_object())
                                            .and_then(|map| {
                                                map.iter().find_map(|(k, v)| {
                                                    v.as_str().map(|value| {
                                                        (
                                                            k.trim().to_string(),
                                                            value.trim().to_string(),
                                                        )
                                                    })
                                                })
                                            })
                                            .unwrap_or_else(|| (String::new(), String::new()));

                                        if name.is_empty() && url.is_empty() {
                                            None
                                        } else {
                                            Some(UiHttpCallServer {
                                                name,
                                                url,
                                                header_key,
                                                header_value,
                                            })
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        if parsed_servers.is_empty() {
                            let (header_key, header_value) = http_call_cfg
                                .get("custom_headers")
                                .and_then(|v| v.as_object())
                                .and_then(|map| {
                                    map.iter().find_map(|(k, v)| {
                                        v.as_str().map(|value| {
                                            (k.trim().to_string(), value.trim().to_string())
                                        })
                                    })
                                })
                                .or_else(|| {
                                    http_call_cfg
                                        .get("default_headers")
                                        .and_then(|v| v.as_object())
                                        .and_then(|map| {
                                            map.iter().find_map(|(k, v)| {
                                                v.as_str().map(|value| {
                                                    (k.trim().to_string(), value.trim().to_string())
                                                })
                                            })
                                        })
                                })
                                .unwrap_or_else(|| (String::new(), String::new()));

                            if let Some(base_urls) =
                                http_call_cfg.get("base_urls").and_then(|v| v.as_array())
                            {
                                parsed_servers = base_urls
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(index, value)| {
                                        let url =
                                            value.as_str().unwrap_or_default().trim().to_string();
                                        if url.is_empty() {
                                            None
                                        } else {
                                            Some(UiHttpCallServer {
                                                name: format!("server_{}", index + 1),
                                                url,
                                                header_key: header_key.clone(),
                                                header_value: header_value.clone(),
                                            })
                                        }
                                    })
                                    .collect::<Vec<_>>();
                            }

                            if parsed_servers.is_empty() {
                                let base_url = http_call_cfg
                                    .get("base_url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                if !base_url.is_empty() {
                                    parsed_servers.push(UiHttpCallServer {
                                        name: "default".to_string(),
                                        url: base_url,
                                        header_key,
                                        header_value,
                                    });
                                }
                            }
                        }

                        http_call_servers_form.set(parsed_servers);
                    }

                    if let Some(search_cfg) = tools_value.get("search_internet") {
                        if let Some(provider) = search_cfg.get("provider").and_then(|v| v.as_str())
                        {
                            let normalized = match provider {
                                "openai" | "grok" | "perplexity" => provider,
                                _ => "openai",
                            };
                            search_provider.set(normalized.to_string());
                        }
                        if let Some(model) = search_cfg.get("model").and_then(|v| v.as_str()) {
                            search_model.set(model.to_string());
                        }
                        if let Some(citations) =
                            search_cfg.get("citations").and_then(|v| v.as_bool())
                        {
                            search_citations.set(citations);
                        }
                        if let Some(web) =
                            search_cfg.get("grok_web_search").and_then(|v| v.as_bool())
                        {
                            search_grok_web.set(web);
                        }
                        if let Some(x_search) =
                            search_cfg.get("grok_x_search").and_then(|v| v.as_bool())
                        {
                            search_grok_x.set(x_search);
                        }
                        if let Some(timeout) =
                            search_cfg.get("grok_timeout").and_then(|v| v.as_u64())
                        {
                            search_grok_timeout.set(timeout.to_string());
                        }
                        if let Some(perms) = search_cfg.get("permissions") {
                            if allowlist.is_empty() {
                                if let Some(items) =
                                    perms.get("network_allow").and_then(|v| v.as_array())
                                {
                                    allowlist = items
                                        .iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect();
                                }
                            }
                        }
                    }

                    if let Some(reminders_cfg) = tools_value.get("reminders") {
                        if let Some(path) =
                            reminders_cfg.get("sqlite_path").and_then(|v| v.as_str())
                        {
                            reminders_sqlite_path.set(path.to_string());
                        }
                    }
                }

                match &config.prompt_source {
                    crate::config::MarkdownSource::Url { url } => {
                        context_path.set(url.clone());
                        match load_markdown_source(url).await {
                            Ok(text) => context_text.set(text),
                            Err(err) => context_error.set(format!("Prompt URL error: {err}")),
                        }
                    }
                    crate::config::MarkdownSource::Database { markdown } => {
                        context_path.set("database".to_string());
                        context_text.set(markdown.clone());
                    }
                }

                let mut heartbeat_error = heartbeat_error;
                match &config.heartbeat_source {
                    crate::config::MarkdownSource::Url { url } => {
                        heartbeat_path.set(url.clone());
                        match load_markdown_source(url).await {
                            Ok(text) => heartbeat_text.set(text),
                            Err(err) => heartbeat_error.set(format!("Heartbeat URL error: {err}")),
                        }
                    }
                    crate::config::MarkdownSource::Database { markdown } => {
                        heartbeat_path.set("database".to_string());
                        heartbeat_text.set(markdown.clone());
                    }
                }

                search_default_deny.set(default_deny);
                if !allowlist.is_empty() {
                    search_network_allow.set(allowlist.join(", "));
                }
                network_allow_form.set(allowlist);

                match crate::vault::get_secret("github_pat") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        github_pat_input.set(secret);
                    }
                    Ok(_) => github_pat_input.set(String::new()),
                    Err(err) => settings_error.set(format!("Vault error: {err}")),
                }

                match crate::vault::get_secret("zapier_token") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        zapier_token_input.set(secret);
                    }
                    Ok(_) => zapier_token_input.set(String::new()),
                    Err(err) => settings_error.set(format!("Vault error: {err}")),
                }

                match crate::vault::get_secret("coding_openai_api_key") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        coding_api_key_input.set(secret);
                    }
                    Ok(_) => coding_api_key_input.set(String::new()),
                    Err(err) => settings_error.set(format!("Vault error: {err}")),
                }

                let provider_name = search_provider();
                let secret_name = match provider_name.as_str() {
                    "perplexity" => "search_internet_perplexity_api_key",
                    "grok" => "search_internet_grok_api_key",
                    _ => "search_internet_openai_api_key",
                };
                match crate::vault::get_secret(secret_name) {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        search_api_key_input.set(secret);
                        search_api_key_status.set("Stored in vault".to_string());
                    }
                    Ok(_) => {
                        let fallback = config
                            .tools
                            .as_ref()
                            .and_then(|tools| tools.get("search_internet"))
                            .and_then(|search| search.get("api_key"))
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                        search_api_key_input.set(fallback);
                        search_api_key_status.set("Not set".to_string());
                    }
                    Err(err) => {
                        search_api_key_status.set(format!("Vault error: {err}"));
                    }
                }

                if !*daemon_running.read() {
                    boot_status.set(
                        "Daemon is stopped. Start it to preload prompt + heartbeat.".to_string(),
                    );
                } else {
                    // Preload prompt into memory and heartbeat into agent.
                    boot_ready.set(true);
                    boot_status.set("Initializing prompt + heartbeat in background...".to_string());
                    let client = reqwest::Client::new();
                    let url = format!("{}/preload_boot", daemon_url().trim_end_matches('/'));
                    let mut request = client
                        .post(&url)
                        .json(&PreloadBootRequest { user_id: user_id() });
                    let token_value = token();
                    if !token_value.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token_value}"));
                    }
                    match request.send().await {
                        Ok(resp) if resp.status().is_success() => {
                            boot_status.set("Boot preload started in background...".to_string());
                        }
                        Ok(resp) => {
                            let status = resp.status();
                            boot_status.set(format!(
                                "Boot preload failed: HTTP {status}. Continuing without preload."
                            ));
                        }
                        Err(err) => {
                            boot_status.set(format!(
                                "Boot preload error: {err}. Continuing without preload."
                            ));
                        }
                    }
                }

                tools_loaded.set(true);
            });
        });
    }

    let on_save_config = {
        let settings_error = settings_error.clone();
        let settings_status = settings_status.clone();
        let config_json_text = config_json_text.clone();
        let wakeup_poll_seconds_input = wakeup_poll_seconds_input.clone();
        let github_pat_input = github_pat_input.clone();
        let zapier_token_input = zapier_token_input.clone();
        let coding_api_key_input = coding_api_key_input.clone();
        let search_provider = search_provider.clone();
        let search_api_key_input = search_api_key_input.clone();
        let mcp_servers_form = mcp_servers_form.clone();
        let http_call_servers_form = http_call_servers_form.clone();
        let network_allow_form = network_allow_form.clone();
        let db_path = db_path.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();

        use_callback(move |_: ()| {
            let settings_error = settings_error.clone();
            let settings_status = settings_status.clone();
            let config_json_text = config_json_text.clone();
            let wakeup_poll_seconds_input = wakeup_poll_seconds_input.clone();
            let github_pat_input = github_pat_input.clone();
            let zapier_token_input = zapier_token_input.clone();
            let coding_api_key_input = coding_api_key_input.clone();
            let search_provider = search_provider.clone();
            let search_api_key_input = search_api_key_input.clone();
            let mcp_servers_form = mcp_servers_form.clone();
            let http_call_servers_form = http_call_servers_form.clone();
            let network_allow_form = network_allow_form.clone();
            let db_path = db_path.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut settings_status = settings_status;
                let mut config_json_text = config_json_text;

                settings_error.set(String::new());
                settings_status.set(String::new());

                let wakeup_poll_seconds = match wakeup_poll_seconds_input().trim().parse::<u64>() {
                    Ok(value) if value > 0 => value,
                    _ => {
                        settings_error
                            .set("Wakeup interval must be a number greater than 0.".to_string());
                        return;
                    }
                };

                let mut mcp_servers = Vec::new();
                for entry in mcp_servers_form().iter() {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    if name.is_empty() && url.is_empty() {
                        continue;
                    }
                    if name.is_empty() || url.is_empty() {
                        settings_error
                            .set("Each MCP server needs both a name and URL.".to_string());
                        return;
                    }
                    if !url.starts_with("http://") && !url.starts_with("https://") {
                        settings_error.set(format!(
                            "MCP server URL must start with http:// or https:// ({url})."
                        ));
                        return;
                    }
                    if (header_key.is_empty() && !header_value.is_empty())
                        || (!header_key.is_empty() && header_value.is_empty())
                    {
                        settings_error.set(
                            "MCP header key and value must both be set or both be empty."
                                .to_string(),
                        );
                        return;
                    }
                    mcp_servers.push((
                        name.to_string(),
                        url.to_string(),
                        header_key.to_string(),
                        header_value.to_string(),
                    ));
                }

                let mut http_call_servers = Vec::new();
                for entry in http_call_servers_form().iter() {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    if name.is_empty() && url.is_empty() {
                        continue;
                    }
                    if name.is_empty() || url.is_empty() {
                        settings_error
                            .set("Each HTTP call server needs both a name and URL.".to_string());
                        return;
                    }
                    if !url.starts_with("http://") && !url.starts_with("https://") {
                        settings_error.set(format!(
                            "HTTP call server URL must start with http:// or https:// ({url})."
                        ));
                        return;
                    }
                    if (header_key.is_empty() && !header_value.is_empty())
                        || (!header_key.is_empty() && header_value.is_empty())
                    {
                        settings_error.set(
                            "HTTP call header key and value must both be set or both be empty."
                                .to_string(),
                        );
                        return;
                    }

                    http_call_servers.push((
                        name.to_string(),
                        url.to_string(),
                        header_key.to_string(),
                        header_value.to_string(),
                    ));
                }

                let network_allow = network_allow_form()
                    .iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>();

                let search_provider_value = match search_provider().trim() {
                    "openai" | "grok" | "perplexity" => search_provider().trim().to_string(),
                    _ => {
                        settings_error.set(
                            "Search provider must be openai, grok, or perplexity.".to_string(),
                        );
                        return;
                    }
                };

                let mut config = match crate::config::Config::from_store(&db_path) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Failed to load current config: {err}"));
                        return;
                    }
                };

                let tools_value = config
                    .tools
                    .get_or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !tools_value.is_object() {
                    *tools_value = Value::Object(serde_json::Map::new());
                }
                let Some(tools_obj) = tools_value.as_object_mut() else {
                    settings_error.set("Failed to update tools config.".to_string());
                    return;
                };

                let wakeup_cfg = tools_obj
                    .entry("wakeup")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !wakeup_cfg.is_object() {
                    *wakeup_cfg = Value::Object(serde_json::Map::new());
                }
                if let Some(wakeup_obj) = wakeup_cfg.as_object_mut() {
                    wakeup_obj.insert(
                        "poll_seconds".to_string(),
                        Value::Number(serde_json::Number::from(wakeup_poll_seconds)),
                    );
                }

                let mcp_cfg = tools_obj
                    .entry("mcp")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !mcp_cfg.is_object() {
                    *mcp_cfg = Value::Object(serde_json::Map::new());
                }
                if let Some(mcp_obj) = mcp_cfg.as_object_mut() {
                    mcp_obj.insert(
                        "servers".to_string(),
                        Value::Array(
                            mcp_servers
                                .into_iter()
                                .map(|(name, url, header_key, header_value)| {
                                    let mut server = serde_json::Map::new();
                                    server.insert("name".to_string(), Value::String(name));
                                    server.insert("url".to_string(), Value::String(url));
                                    if !header_key.is_empty() {
                                        let mut headers = serde_json::Map::new();
                                        headers.insert(header_key, Value::String(header_value));
                                        server
                                            .insert("headers".to_string(), Value::Object(headers));
                                    }
                                    Value::Object(server)
                                })
                                .collect(),
                        ),
                    );
                }

                let http_call_cfg = tools_obj
                    .entry("http_call")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !http_call_cfg.is_object() {
                    *http_call_cfg = Value::Object(serde_json::Map::new());
                }
                if let Some(http_call_obj) = http_call_cfg.as_object_mut() {
                    let mut first_url: Option<String> = None;
                    let mut first_header: Option<(String, String)> = None;

                    let servers = http_call_servers
                        .iter()
                        .map(|(name, url, header_key, header_value)| {
                            if first_url.is_none() {
                                first_url = Some(url.clone());
                            }
                            let mut server = serde_json::Map::new();
                            server.insert("name".to_string(), Value::String(name.clone()));
                            server.insert("url".to_string(), Value::String(url.clone()));
                            if !header_key.is_empty() {
                                if first_header.is_none() {
                                    first_header = Some((header_key.clone(), header_value.clone()));
                                }
                                let mut headers = serde_json::Map::new();
                                headers.insert(
                                    header_key.clone(),
                                    Value::String(header_value.clone()),
                                );
                                server.insert("headers".to_string(), Value::Object(headers));
                            }
                            Value::Object(server)
                        })
                        .collect::<Vec<_>>();

                    http_call_obj.insert("servers".to_string(), Value::Array(servers));

                    let base_urls = http_call_servers
                        .iter()
                        .map(|(_, url, _, _)| Value::String(url.clone()))
                        .collect::<Vec<_>>();
                    http_call_obj.insert("base_urls".to_string(), Value::Array(base_urls));

                    if let Some(url) = first_url {
                        http_call_obj.insert("base_url".to_string(), Value::String(url));
                    } else {
                        http_call_obj.remove("base_url");
                    }

                    let mut legacy_headers = serde_json::Map::new();
                    if let Some((header_key, header_value)) = first_header {
                        legacy_headers.insert(header_key, Value::String(header_value));
                    }
                    http_call_obj.insert(
                        "custom_headers".to_string(),
                        Value::Object(legacy_headers.clone()),
                    );
                    http_call_obj
                        .insert("default_headers".to_string(), Value::Object(legacy_headers));
                }

                let search_cfg = tools_obj
                    .entry("search_internet")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !search_cfg.is_object() {
                    *search_cfg = Value::Object(serde_json::Map::new());
                }
                if let Some(search_obj) = search_cfg.as_object_mut() {
                    search_obj.insert(
                        "provider".to_string(),
                        Value::String(search_provider_value.clone()),
                    );
                    search_obj.remove("api_key");
                }

                let settings_cfg = tools_obj
                    .entry("settings")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !settings_cfg.is_object() {
                    *settings_cfg = Value::Object(serde_json::Map::new());
                }
                if let Some(settings_obj) = settings_cfg.as_object_mut() {
                    let permissions = settings_obj
                        .entry("permissions")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if !permissions.is_object() {
                        *permissions = Value::Object(serde_json::Map::new());
                    }
                    if let Some(perms_obj) = permissions.as_object_mut() {
                        perms_obj.insert(
                            "network_allow".to_string(),
                            Value::Array(
                                network_allow
                                    .into_iter()
                                    .map(Value::String)
                                    .collect::<Vec<_>>(),
                            ),
                        );
                    }
                }

                let github_pat = github_pat_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret("github_pat", &github_pat) {
                    settings_error.set(format!("Failed to store GitHub token: {err}"));
                    return;
                }

                let zapier_token = zapier_token_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret("zapier_token", &zapier_token) {
                    settings_error.set(format!("Failed to store Zapier token: {err}"));
                    return;
                }

                let coding_api_key = coding_api_key_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret("coding_openai_api_key", &coding_api_key)
                {
                    settings_error.set(format!("Failed to store coding API key: {err}"));
                    return;
                }

                let search_secret_name = match search_provider_value.as_str() {
                    "perplexity" => "search_internet_perplexity_api_key",
                    "grok" => "search_internet_grok_api_key",
                    _ => "search_internet_openai_api_key",
                };
                let search_api_key = search_api_key_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret(search_secret_name, &search_api_key) {
                    settings_error.set(format!("Failed to store search API key: {err}"));
                    return;
                }

                let pretty = match serde_json::to_string_pretty(&config) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Failed to serialize config: {err}"));
                        return;
                    }
                };

                if let Err(err) = crate::vault::set_secret("app_config_json", &pretty) {
                    settings_error.set(format!("Failed to store config in keyring: {err}"));
                    return;
                }

                let config_for_save = config.clone();
                let db_path_for_save = db_path.clone();

                let result = tokio::task::spawn_blocking(move || {
                    crate::config_store::save_config(&db_path_for_save, &config_for_save)
                })
                .await;

                match result {
                    Ok(Ok(())) => {
                        config_json_text.set(pretty);
                        let client = reqwest::Client::new();
                        let url = format!("{}/reload_config", daemon_url().trim_end_matches('/'));
                        let mut request = client.post(url);
                        let token_value = token();
                        if !token_value.trim().is_empty() {
                            request =
                                request.header("authorization", format!("Bearer {token_value}"));
                        }
                        match request.send().await {
                            Ok(response) if response.status().is_success() => {
                                settings_status.set("Settings saved.".to_string());
                            }
                            Ok(response) => {
                                let status = response.status();
                                let text = response
                                    .text()
                                    .await
                                    .unwrap_or_else(|_| "Unable to read error body".to_string());
                                settings_status.set(format!(
                                    "Settings saved, but reload failed ({status}). Restart required. {text}"
                                ));
                            }
                            Err(err) => {
                                settings_status.set(format!(
                                    "Settings saved, but reload failed: {err}. Restart required."
                                ));
                            }
                        }
                    }
                    Ok(Err(err)) => settings_error.set(format!("Save failed: {err}")),
                    Err(err) => settings_error.set(format!("Save failed: {err}")),
                }
            });
        })
    };

    let on_validate_context = {
        let context_text = context_text.clone();
        let context_path = context_path.clone();
        let context_status = context_status.clone();
        let context_error = context_error.clone();

        use_callback(move |_: ()| {
            let context_text = context_text.clone();
            let context_path = context_path.clone();
            let context_status = context_status.clone();
            let context_error = context_error.clone();

            spawn(async move {
                let mut context_status = context_status;
                let mut context_error = context_error;

                context_status.set(String::new());
                context_error.set(String::new());

                let source = context_path();
                if source.trim().is_empty() {
                    context_error.set("Context file path is empty.".to_string());
                    return;
                }

                if is_url_source(&source) {
                    match load_markdown_source(&source).await {
                        Ok(text) if !text.trim().is_empty() => {
                            context_status.set("Context URL is reachable.".to_string())
                        }
                        Ok(_) => {
                            context_error.set("Context URL returned empty content.".to_string())
                        }
                        Err(err) => context_error.set(format!("Context URL error: {err}")),
                    }
                    return;
                }

                let content = context_text();
                if content.trim().is_empty() {
                    context_error.set("Context markdown is empty.".to_string());
                    return;
                }
                context_status.set("Context markdown looks valid.".to_string());
            });
        })
    };

    let on_save_context = {
        let context_text = context_text.clone();
        let context_path = context_path.clone();
        let context_status = context_status.clone();
        let context_error = context_error.clone();
        let db_path = db_path.clone();
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();

        use_callback(move |_: ()| {
            let context_text = context_text.clone();
            let context_path = context_path.clone();
            let context_status = context_status.clone();
            let context_error = context_error.clone();
            let db_path = db_path.clone();
            let daemon_running = daemon_running.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();

            spawn(async move {
                let mut context_status = context_status;
                let mut context_error = context_error;

                context_status.set(String::new());
                context_error.set(String::new());

                let source = context_path();
                if source.trim().is_empty() {
                    context_error.set("Context source is empty.".to_string());
                    return;
                }
                if is_url_source(&source) {
                    context_error.set(
                        "Context source is a URL and cannot be saved in the DB editor.".to_string(),
                    );
                    return;
                }

                let content = context_text();
                if content.trim().is_empty() {
                    context_error.set("Context markdown is empty.".to_string());
                    return;
                }

                if let Err(err) = save_markdown_source_to_store(
                    db_path.clone(),
                    "prompt",
                    crate::config::MarkdownSource::Database { markdown: content },
                )
                .await
                {
                    context_error.set(format!("Failed to save context to DB: {err}"));
                    return;
                }

                if *daemon_running.read() {
                    let client = reqwest::Client::new();
                    let url = format!("{}/reload_config", daemon_url().trim_end_matches('/'));
                    let mut request = client.post(url);
                    let token_value = token();
                    if !token_value.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token_value}"));
                    }
                    match request.send().await {
                        Ok(response) if response.status().is_success() => {
                            context_status.set("Context saved to DB and runtime reloaded.".to_string())
                        }
                        Ok(response) => {
                            context_status.set(format!(
                                "Context saved to DB. Reload failed (HTTP {}). Restart daemon to apply.",
                                response.status()
                            ))
                        }
                        Err(err) => {
                            context_status.set(format!(
                                "Context saved to DB. Reload failed: {err}. Restart daemon to apply."
                            ))
                        }
                    }
                } else {
                    context_status.set("Context saved to DB.".to_string());
                }
            });
        })
    };

    let on_validate_heartbeat = {
        let heartbeat_text = heartbeat_text.clone();
        let heartbeat_path = heartbeat_path.clone();
        let heartbeat_status = heartbeat_status.clone();
        let heartbeat_error = heartbeat_error.clone();

        use_callback(move |_: ()| {
            let heartbeat_text = heartbeat_text.clone();
            let heartbeat_path = heartbeat_path.clone();
            let heartbeat_status = heartbeat_status.clone();
            let heartbeat_error = heartbeat_error.clone();

            spawn(async move {
                let mut heartbeat_status = heartbeat_status;
                let mut heartbeat_error = heartbeat_error;

                heartbeat_status.set(String::new());
                heartbeat_error.set(String::new());

                let source = heartbeat_path();
                if source.trim().is_empty() {
                    heartbeat_error.set("Heartbeat path or URL is empty.".to_string());
                    return;
                }

                if is_url_source(&source) {
                    match load_markdown_source(&source).await {
                        Ok(text) if !text.trim().is_empty() => {
                            heartbeat_status.set("Heartbeat URL is reachable.".to_string())
                        }
                        Ok(_) => {
                            heartbeat_error.set("Heartbeat URL returned empty content.".to_string())
                        }
                        Err(err) => heartbeat_error.set(format!("Heartbeat URL error: {err}")),
                    }
                    return;
                }

                let content = heartbeat_text();
                if content.trim().is_empty() {
                    heartbeat_error.set("Heartbeat markdown is empty.".to_string());
                    return;
                }
                heartbeat_status.set("Heartbeat markdown looks valid.".to_string());
            });
        })
    };

    let on_save_heartbeat = {
        let heartbeat_text = heartbeat_text.clone();
        let heartbeat_path = heartbeat_path.clone();
        let heartbeat_status = heartbeat_status.clone();
        let heartbeat_error = heartbeat_error.clone();
        let db_path = db_path.clone();
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();

        use_callback(move |_: ()| {
            let heartbeat_text = heartbeat_text.clone();
            let heartbeat_path = heartbeat_path.clone();
            let heartbeat_status = heartbeat_status.clone();
            let heartbeat_error = heartbeat_error.clone();
            let db_path = db_path.clone();
            let daemon_running = daemon_running.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();

            spawn(async move {
                let mut heartbeat_status = heartbeat_status;
                let mut heartbeat_error = heartbeat_error;

                heartbeat_status.set(String::new());
                heartbeat_error.set(String::new());

                let source = heartbeat_path();
                if source.trim().is_empty() {
                    heartbeat_error.set("Heartbeat source is empty.".to_string());
                    return;
                }
                if is_url_source(&source) {
                    heartbeat_error.set(
                        "Heartbeat source is a URL and cannot be saved in the DB editor."
                            .to_string(),
                    );
                    return;
                }

                let content = heartbeat_text();
                if content.trim().is_empty() {
                    heartbeat_error.set("Heartbeat markdown is empty.".to_string());
                    return;
                }

                if let Err(err) = save_markdown_source_to_store(
                    db_path.clone(),
                    "heartbeat",
                    crate::config::MarkdownSource::Database { markdown: content },
                )
                .await
                {
                    heartbeat_error.set(format!("Failed to save heartbeat to DB: {err}"));
                    return;
                }

                if *daemon_running.read() {
                    let client = reqwest::Client::new();
                    let url = format!("{}/reload_config", daemon_url().trim_end_matches('/'));
                    let mut request = client.post(url);
                    let token_value = token();
                    if !token_value.trim().is_empty() {
                        request = request.header("authorization", format!("Bearer {token_value}"));
                    }
                    match request.send().await {
                        Ok(response) if response.status().is_success() => {
                            heartbeat_status
                                .set("Heartbeat saved to DB and runtime reloaded.".to_string())
                        }
                        Ok(response) => {
                            heartbeat_status.set(format!(
                                "Heartbeat saved to DB. Reload failed (HTTP {}). Restart daemon to apply.",
                                response.status()
                            ))
                        }
                        Err(err) => {
                            heartbeat_status.set(format!(
                                "Heartbeat saved to DB. Reload failed: {err}. Restart daemon to apply."
                            ))
                        }
                    }
                } else {
                    heartbeat_status.set("Heartbeat saved to DB.".to_string());
                }
            });
        })
    };

    let active_tab_chat = active_tab.clone();
    let active_tab_activity = active_tab.clone();
    let active_tab_config = active_tab.clone();
    let active_tab_context = active_tab.clone();
    let active_tab_heartbeat = active_tab.clone();
    let message_input = input.clone();

    rsx! {
        style { r#"
            body {{
                font-family: system-ui, -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", sans-serif;
                background: radial-gradient(1200px 800px at 20% -10%, rgba(120,119,198,0.35), transparent 60%),
                            radial-gradient(1000px 700px at 110% 10%, rgba(56,189,248,0.25), transparent 60%),
                            #0b1020;
                color: #e5e7eb;
                margin: 0;
                overflow: hidden;
            }}
            .container {{ max-width: 980px; margin: 0 auto; padding: 10px; height: 100dvh; box-sizing: border-box; overflow: hidden; display: flex; flex-direction: column; gap: 10px; }}
            .header {{
                padding: 16px 20px;
                background: rgba(17,24,39,0.55);
                color: #e5e7eb;
                display: flex; align-items: center; justify-content: space-between;
                border-bottom: 1px solid rgba(255,255,255,0.08);
                backdrop-filter: blur(18px) saturate(180%);
                box-shadow: 0 8px 30px rgba(0,0,0,0.25);
                border-radius: 14px;
            }}
            .nav {{ display: flex; gap: 8px; align-items: center; }}
            .nav button {{ background: rgba(255,255,255,0.08); }}
            .nav button.active {{ background: rgba(99,102,241,0.6); }}
            .nav-controls {{ display: flex; gap: 6px; margin-left: 8px; }}
            .daemon-icon-btn {{
                width: 36px;
                height: 36px;
                min-width: 36px;
                padding: 0;
                border-radius: 999px;
                display: inline-flex;
                align-items: center;
                justify-content: center;
                font-size: 16px;
                line-height: 1;
                background: rgba(255,255,255,0.12);
            }}
            .daemon-icon-btn:disabled {{ opacity: 0.45; }}
            .daemon-trash-btn {{
                margin-left: 6px;
                background: rgba(239,68,68,0.30);
            }}
            .daemon-trash-btn:hover {{
                background: rgba(239,68,68,0.50);
            }}
            .title {{ display: flex; align-items: center; }}
            .title-logo {{ width: 30px; height: 30px; display: block; }}
            .chat {{ flex: 1; min-height: 0; overflow-y: auto; padding: 20px; background: rgba(10,16,34,0.22); border: 1px solid rgba(255,255,255,0.08); border-radius: 16px; }}
            .bubble {{
                max-width: 72%;
                padding: 12px 14px;
                border-radius: 18px;
                margin-bottom: 10px;
                white-space: pre-wrap;
                overflow-wrap: anywhere;
                word-break: break-word;
                line-height: 1.45;
                background: rgba(255,255,255,0.10);
                border: 1px solid rgba(255,255,255,0.12);
                backdrop-filter: blur(14px) saturate(180%);
                box-shadow: inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 30px rgba(0,0,0,0.18);
            }}
            .bubble.user {{ margin-left: auto; background: rgba(99,102,241,0.55); color: white; border-bottom-right-radius: 6px; }}
            .bubble.bot {{ margin-right: auto; background: rgba(124,58,237,0.45); color: white; border-bottom-left-radius: 6px; }}
            .bubble-content {{ margin-bottom: 6px; }}
            .bubble-time {{ font-size: 11px; color: rgba(229,231,235,0.72); letter-spacing: 0.02em; }}
            .bubble.user .bubble-time {{ text-align: right; }}
            .bubble.bot .bubble-time {{ text-align: right; }}
            .composer {{
                padding: 16px 20px;
                background: rgba(17,24,39,0.55);
                border-top: 1px solid rgba(255,255,255,0.08);
                display: flex; flex-direction: column; gap: 12px;
                position: sticky; bottom: 0;
                backdrop-filter: blur(18px) saturate(180%);
                border-radius: 16px;
            }}
            .composer-row {{ display: flex; flex-direction: column; gap: 8px; }}
            .composer-input {{ position: relative; display: flex; align-items: stretch; }}
            textarea {{
                flex: 1;
                min-height: 52px;
                max-height: 200px;
                resize: vertical;
                padding-right: 60px;
                white-space: pre-wrap;
                overflow-wrap: anywhere;
                word-break: break-word;
            }}
            label {{ display: block; font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: rgba(229,231,235,0.7); margin-bottom: 6px; }}
            input, textarea, select {{
                width: 100%; padding: 10px 12px; border-radius: 12px;
                border: 1px solid rgba(255,255,255,0.12);
                background: rgba(15,23,42,0.55);
                color: #e5e7eb;
                backdrop-filter: blur(12px) saturate(180%);
                box-shadow: inset 0 1px 0 rgba(255,255,255,0.06);
            }}
            input:focus, textarea:focus, select:focus {{
                outline: none;
                border-color: rgba(99,102,241,0.75);
                background: rgba(15,23,42,0.9);
                color: #e5e7eb;
            }}
            select {{
                appearance: none;
                -webkit-appearance: none;
                -moz-appearance: none;
                background: rgba(15,23,42,0.9);
                color: #e5e7eb;
                padding-right: 36px;
                background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='none'%3E%3Cpath d='M6 8l4 4 4-4' stroke='%23e5e7eb' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
                background-repeat: no-repeat;
                background-position: right 12px center;
                background-size: 14px 14px;
            }}
            select option {{
                background: #0f172a;
                color: #e5e7eb;
            }}
            .config-editor {{
                font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
                font-size: 13px;
                line-height: 1.5;
                background: rgba(2,6,23,0.6);
                border: 1px solid rgba(148,163,184,0.35);
                border-radius: 14px;
                padding: 14px 16px;
                min-height: 340px;
            }}
            .config-grid {{
                display: grid;
                grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
                gap: 14px;
                align-items: stretch;
            }}
            .config-head {{
                display: grid;
                grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
                gap: 14px;
            }}
            .config-head label {{
                margin-bottom: 6px;
                line-height: 16px;
            }}
            .config-panel {{
                display: flex;
                flex-direction: column;
                min-width: 0;
                min-height: 420px;
                height: 100%;
            }}
            .config-panel > textarea,
            .config-panel > pre {{
                height: 100%;
                width: 100%;
            }}
            .config-panel > pre {{
                margin: 0;
            }}
            .config-preview {{
                font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
                font-size: 13px;
                line-height: 1.5;
                background: rgba(2,6,23,0.65);
                border: 1px solid rgba(148,163,184,0.35);
                border-radius: 14px;
                padding: 14px 16px;
                overflow: auto;
                white-space: pre-wrap;
                overflow-wrap: anywhere;
                box-sizing: border-box;
                margin: 0;
            }}
            .config-editor {{
                resize: none;
                box-sizing: border-box;
                height: 100%;
                max-height: 100%;
            }}
            @media (max-width: 860px) {{
                .config-grid {{ grid-template-columns: 1fr; }}
                .config-head {{ grid-template-columns: 1fr; }}
                .config-panel {{ min-height: 360px; }}
            }}
            .config-actions {{
                display: flex;
                flex-wrap: wrap;
                gap: 10px;
                align-items: center;
                justify-content: center;
                margin-top: 24px;
            }}
            .config-actions button {{
                min-width: 140px;
            }}
            button {{
                padding: 10px 18px; border-radius: 16px; border: 1px solid rgba(255,255,255,0.12);
                background: rgba(99,102,241,0.55);
                color: white; font-weight: 600; cursor: pointer;
                backdrop-filter: blur(14px) saturate(180%);
                box-shadow: inset 0 1px 0 rgba(255,255,255,0.08), 0 10px 24px rgba(0,0,0,0.18);
                transition: transform 0.08s ease, box-shadow 0.2s ease, background 0.2s ease;
            }}
            button:hover {{ background: rgba(99,102,241,0.7); }}
            button:active {{ transform: translateY(1px); }}
            button:disabled {{ opacity: 0.6; cursor: not-allowed; }}
            .send {{
                position: absolute;
                right: 6px;
                bottom: 6px;
                height: 40px;
                width: 40px;
                min-width: 40px;
                padding: 0;
                border-radius: 10px;
                display: flex; align-items: center; justify-content: center;
            }}
            .error {{ color: #fca5a5; font-weight: 600; padding: 8px 20px; background: rgba(17,24,39,0.55); backdrop-filter: blur(12px); }}
            .hint {{ color: rgba(229,231,235,0.7); font-size: 12px; }}
            .bubble pre {{ background: rgba(0,0,0,0.2); padding: 10px; border-radius: 10px; overflow-x: auto; }}
            .bubble code {{ font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; }}
            .bubble a {{ color: #e0e7ff; text-decoration: underline; }}
            .bubble blockquote {{ border-left: 3px solid rgba(255,255,255,0.5); margin: 6px 0; padding-left: 10px; color: rgba(255,255,255,0.9); }}
            .bubble ul, .bubble ol {{ padding-left: 20px; margin: 6px 0; }}
            .bubble h1, .bubble h2, .bubble h3 {{ margin: 6px 0; font-weight: 700; }}
            .settings {{ flex: 1; overflow-y: auto; padding: 20px; display: flex; flex-direction: column; gap: 16px; }}
            .settings-card {{
                background: rgba(17,24,39,0.55);
                border: 1px solid rgba(255,255,255,0.12);
                border-radius: 16px;
                padding: 16px;
                backdrop-filter: blur(14px) saturate(180%);
                box-shadow: inset 0 1px 0 rgba(255,255,255,0.06), 0 12px 28px rgba(0,0,0,0.18);
            }}
            .tool-list {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px; }}
            .tool-item {{ display: flex; align-items: center; gap: 10px; }}
            .status {{ color: rgba(229,231,235,0.7); font-weight: 500; }}
            .simple-settings {{
                display: flex;
                flex-direction: column;
                gap: 18px;
            }}
            .simple-top {{
                display: grid;
                grid-template-columns: repeat(3, minmax(0, 1fr));
                gap: 14px;
            }}
            .simple-top > div {{
                min-width: 0;
            }}
            .simple-top input {{
                box-sizing: border-box;
            }}
            .simple-section {{
                display: flex;
                flex-direction: column;
                gap: 10px;
                padding-top: 6px;
            }}
            .simple-list {{
                display: flex;
                flex-direction: column;
                gap: 10px;
            }}
            .simple-row {{
                display: flex;
                align-items: center;
                gap: 10px;
            }}
            .simple-row input {{
                margin: 0;
            }}
            .simple-row button {{
                min-width: 108px;
                padding: 10px 12px;
                border-radius: 12px;
            }}
            .simple-row .mcp-name {{
                flex: 0 0 190px;
            }}
            .simple-row .mcp-url {{
                flex: 1;
            }}
            .simple-row .mcp-header-key {{
                flex: 0 0 180px;
            }}
            .simple-row .mcp-header-value {{
                flex: 0 0 220px;
            }}
            .simple-row .host-input {{
                flex: 1;
            }}
            .simple-actions {{
                display: flex;
                justify-content: center;
                margin-top: 2px;
            }}
            .add-host-actions {{
                margin-top: 12px;
            }}
            @media (max-width: 860px) {{
                .simple-top {{ grid-template-columns: 1fr; }}
                .simple-row {{ flex-direction: column; align-items: stretch; }}
                .simple-row .mcp-name {{ flex: 1 1 auto; }}
                .simple-row .mcp-url {{ flex: 1 1 auto; }}
                .simple-row .mcp-header-key {{ flex: 1 1 auto; }}
                .simple-row .mcp-header-value {{ flex: 1 1 auto; }}
                .simple-row button {{ width: 100%; }}
            }}
        "# }
        div { class: "container",
            div { class: "header",
                div { class: "title",
                    img { class: "title-logo", src: APP_LOGO, alt: "Butterfly Bot" }
                }
                div { class: "nav",
                    button {
                        class: if *active_tab.read() == UiTab::Chat { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_chat = active_tab_chat.clone();
                            active_tab_chat.set(UiTab::Chat);
                        },
                        "Chat"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Activity { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_activity = active_tab_activity.clone();
                            active_tab_activity.set(UiTab::Activity);
                        },
                        "Activity"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Config { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_config = active_tab_config.clone();
                            active_tab_config.set(UiTab::Config);
                        },
                        "Config"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Context { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_context = active_tab_context.clone();
                            active_tab_context.set(UiTab::Context);
                        },
                        "Context"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Heartbeat { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_heartbeat = active_tab_heartbeat.clone();
                            active_tab_heartbeat.set(UiTab::Heartbeat);
                        },
                        "Heartbeat"
                    }
                    div { class: "nav-controls",
                        button {
                            class: "daemon-icon-btn",
                            title: "Start daemon",
                            onclick: move |_| on_daemon_start.call(()),
                            disabled: *daemon_running.read(),
                            "▶"
                        }
                        button {
                            class: "daemon-icon-btn",
                            title: "Stop daemon",
                            onclick: move |_| on_daemon_stop.call(()),
                            disabled: !*daemon_running.read(),
                            "⏹"
                        }
                        button {
                            class: "daemon-icon-btn daemon-trash-btn",
                            title: "Clear chat and activity history",
                            onclick: move |_| on_clear_histories.call(()),
                            "🗑"
                        }
                    }
                }
            }
            if !error.read().is_empty() {
                div { class: "error", "{error}" }
            }
            if *active_tab.read() == UiTab::Chat {
                if !boot_status.read().is_empty() {
                    div { class: "hint", "{boot_status}" }
                }
                if !daemon_status.read().is_empty() {
                    div { class: "hint", "{daemon_status}" }
                }
                div { class: "chat", id: "chat-scroll",
                    for message in messages
                        .read()
                        .iter()
                        .filter(|msg| msg.role == MessageRole::User || !msg.text.is_empty())
                    {
                        div {
                            class: if message.role == MessageRole::User {
                                "bubble user"
                            } else {
                                "bubble bot"
                            },
                            div {
                                class: "bubble-content",
                                dangerous_inner_html: message.html.clone(),
                            }
                            div { class: "bubble-time", "{format_local_time(message.timestamp)}" }
                        }
                    }
                    if *busy.read() {
                        div { class: "hint", "Bot is typing…" }
                    }
                }
                div { class: "composer",
                    div { class: "composer-row",
                        label { "Message" }
                        div { class: "composer-input",
                            textarea {
                                value: "{input}",
                                oninput: move |evt| {
                                    let mut message_input = message_input.clone();
                                    message_input.set(evt.value());
                                },
                                onkeydown: move |evt| {
                                    if evt.key() == Key::Enter && !evt.modifiers().shift() {
                                        evt.prevent_default();
                                        on_send_key.call(());
                                    }
                                },
                            }
                            button {
                                class: "send",
                                disabled: *busy.read(),
                                onclick: move |_| on_send.call(()),
                                "Send"
                            }
                        }
                    }
                }
            }
            if *active_tab.read() == UiTab::Config {
                div { class: "settings",
                    if !*tools_loaded.read() {
                        div { class: "hint", "Loading config…" }
                    }
                    if *tools_loaded.read() {
                        div { class: "settings-card",
                            label { "Simple Settings" }
                            p { class: "hint", "Only essential settings are editable here." }

                            div { class: "simple-settings",
                                div { class: "simple-top",
                                    div {
                                        label { "Wakeup Interval (seconds)" }
                                        input {
                                            r#type: "number",
                                            min: "1",
                                            value: "{wakeup_poll_seconds_input}",
                                            oninput: move |evt| {
                                                let mut wakeup_poll_seconds_input = wakeup_poll_seconds_input.clone();
                                                wakeup_poll_seconds_input.set(evt.value());
                                            },
                                        }
                                    }
                                    div {
                                        label { "GitHub Token" }
                                        input {
                                            r#type: "password",
                                            value: "{github_pat_input}",
                                            oninput: move |evt| {
                                                let mut github_pat_input = github_pat_input.clone();
                                                github_pat_input.set(evt.value());
                                            },
                                            placeholder: "Paste PAT",
                                        }
                                    }
                                    div {
                                        label { "Zapier Token" }
                                        input {
                                            r#type: "password",
                                            value: "{zapier_token_input}",
                                            oninput: move |evt| {
                                                let mut zapier_token_input = zapier_token_input.clone();
                                                zapier_token_input.set(evt.value());
                                            },
                                            placeholder: "Paste Zapier token",
                                        }
                                    }
                                }

                                div { class: "simple-top",
                                    div {
                                        label { "Coding OpenAI API Key" }
                                        input {
                                            r#type: "password",
                                            value: "{coding_api_key_input}",
                                            oninput: move |evt| {
                                                let mut coding_api_key_input = coding_api_key_input.clone();
                                                coding_api_key_input.set(evt.value());
                                            },
                                            placeholder: "Paste coding API key",
                                        }
                                    }
                                    div {
                                        label { "Search Provider" }
                                        select {
                                            value: "{search_provider()}",
                                            onchange: move |evt| {
                                                let selected = evt.value();
                                                let normalized = match selected.as_str() {
                                                    "openai" | "grok" | "perplexity" => selected,
                                                    _ => "openai".to_string(),
                                                };
                                                let mut search_provider = search_provider.clone();
                                                search_provider.set(normalized.clone());

                                                let secret_name = match normalized.as_str() {
                                                    "perplexity" => "search_internet_perplexity_api_key",
                                                    "grok" => "search_internet_grok_api_key",
                                                    _ => "search_internet_openai_api_key",
                                                };
                                                let mut search_api_key_input = search_api_key_input.clone();
                                                match crate::vault::get_secret(secret_name) {
                                                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                                                        search_api_key_input.set(secret);
                                                    }
                                                    _ => search_api_key_input.set(String::new()),
                                                }
                                            },
                                            option {
                                                value: "openai",
                                                "OpenAI"
                                            }
                                            option {
                                                value: "grok",
                                                "Grok"
                                            }
                                            option {
                                                value: "perplexity",
                                                "Perplexity"
                                            }
                                        }
                                    }
                                    div {
                                        label { "Search API Key" }
                                        input {
                                            r#type: "password",
                                            value: "{search_api_key_input}",
                                            oninput: move |evt| {
                                                let mut search_api_key_input = search_api_key_input.clone();
                                                search_api_key_input.set(evt.value());
                                            },
                                            placeholder: "Paste search API key",
                                        }
                                    }
                                }

                                div { class: "simple-section",
                                    label { "MCP Servers" }
                                    div { class: "simple-list",
                                        for (index, server) in mcp_servers_form.read().iter().enumerate() {
                                            div { class: "simple-row",
                                                input {
                                                    class: "mcp-name",
                                                    value: "{server.name}",
                                                    placeholder: "Server name",
                                                    oninput: move |evt| {
                                                        let mut mcp_servers_form = mcp_servers_form.clone();
                                                        let mut list = mcp_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.name = evt.value();
                                                        }
                                                        mcp_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-url",
                                                    value: "{server.url}",
                                                    placeholder: "https://server.example/mcp",
                                                    oninput: move |evt| {
                                                        let mut mcp_servers_form = mcp_servers_form.clone();
                                                        let mut list = mcp_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.url = evt.value();
                                                        }
                                                        mcp_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-header-key",
                                                    value: "{server.header_key}",
                                                    placeholder: "Header key (optional)",
                                                    oninput: move |evt| {
                                                        let mut mcp_servers_form = mcp_servers_form.clone();
                                                        let mut list = mcp_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.header_key = evt.value();
                                                        }
                                                        mcp_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-header-value",
                                                    value: "{server.header_value}",
                                                    placeholder: "Header value (optional)",
                                                    oninput: move |evt| {
                                                        let mut mcp_servers_form = mcp_servers_form.clone();
                                                        let mut list = mcp_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.header_value = evt.value();
                                                        }
                                                        mcp_servers_form.set(list);
                                                    },
                                                }
                                                button {
                                                    onclick: move |_| {
                                                        let mut mcp_servers_form = mcp_servers_form.clone();
                                                        let mut list = mcp_servers_form();
                                                        if index < list.len() {
                                                            list.remove(index);
                                                        }
                                                        mcp_servers_form.set(list);
                                                    },
                                                    "Remove"
                                                }
                                            }
                                        }
                                    }
                                    div { class: "simple-actions",
                                        button {
                                            onclick: move |_| {
                                                let mut mcp_servers_form = mcp_servers_form.clone();
                                                let mut list = mcp_servers_form();
                                                list.push(UiMcpServer::default());
                                                mcp_servers_form.set(list);
                                            },
                                            "+ Add MCP Server"
                                        }
                                    }
                                }

                                div { class: "simple-section",
                                    label { "HTTP Call Servers" }
                                    div { class: "simple-list",
                                        for (index, server) in http_call_servers_form.read().iter().enumerate() {
                                            div { class: "simple-row",
                                                input {
                                                    class: "mcp-name",
                                                    value: "{server.name}",
                                                    placeholder: "Server name",
                                                    oninput: move |evt| {
                                                        let mut http_call_servers_form = http_call_servers_form.clone();
                                                        let mut list = http_call_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.name = evt.value();
                                                        }
                                                        http_call_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-url",
                                                    value: "{server.url}",
                                                    placeholder: "https://api.example.com/v1",
                                                    oninput: move |evt| {
                                                        let mut http_call_servers_form = http_call_servers_form.clone();
                                                        let mut list = http_call_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.url = evt.value();
                                                        }
                                                        http_call_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-header-key",
                                                    value: "{server.header_key}",
                                                    placeholder: "Header key (optional)",
                                                    oninput: move |evt| {
                                                        let mut http_call_servers_form = http_call_servers_form.clone();
                                                        let mut list = http_call_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.header_key = evt.value();
                                                        }
                                                        http_call_servers_form.set(list);
                                                    },
                                                }
                                                input {
                                                    class: "mcp-header-value",
                                                    value: "{server.header_value}",
                                                    placeholder: "Header value (optional)",
                                                    oninput: move |evt| {
                                                        let mut http_call_servers_form = http_call_servers_form.clone();
                                                        let mut list = http_call_servers_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            item.header_value = evt.value();
                                                        }
                                                        http_call_servers_form.set(list);
                                                    },
                                                }
                                                button {
                                                    onclick: move |_| {
                                                        let mut http_call_servers_form = http_call_servers_form.clone();
                                                        let mut list = http_call_servers_form();
                                                        if index < list.len() {
                                                            list.remove(index);
                                                        }
                                                        http_call_servers_form.set(list);
                                                    },
                                                    "Remove"
                                                }
                                            }
                                        }
                                    }
                                    div { class: "simple-actions",
                                        button {
                                            onclick: move |_| {
                                                let mut http_call_servers_form = http_call_servers_form.clone();
                                                let mut list = http_call_servers_form();
                                                list.push(UiHttpCallServer::default());
                                                http_call_servers_form.set(list);
                                            },
                                            "+ Add HTTP Server"
                                        }
                                    }
                                }

                                div { class: "simple-section",
                                    label { "Network Allow List" }
                                    div { class: "simple-list",
                                        for (index, host) in network_allow_form.read().iter().enumerate() {
                                            div { class: "simple-row",
                                                input {
                                                    class: "host-input",
                                                    value: "{host}",
                                                    placeholder: "api.example.com",
                                                    oninput: move |evt| {
                                                        let mut network_allow_form = network_allow_form.clone();
                                                        let mut list = network_allow_form();
                                                        if let Some(item) = list.get_mut(index) {
                                                            *item = evt.value();
                                                        }
                                                        network_allow_form.set(list);
                                                    },
                                                }
                                                button {
                                                    onclick: move |_| {
                                                        let mut network_allow_form = network_allow_form.clone();
                                                        let mut list = network_allow_form();
                                                        if index < list.len() {
                                                            list.remove(index);
                                                        }
                                                        network_allow_form.set(list);
                                                    },
                                                    "Remove"
                                                }
                                            }
                                        }
                                    }
                                    div { class: "simple-actions add-host-actions",
                                        button {
                                            onclick: move |_| {
                                                let mut network_allow_form = network_allow_form.clone();
                                                let mut list = network_allow_form();
                                                list.push(String::new());
                                                network_allow_form.set(list);
                                            },
                                            "+ Add Host"
                                        }
                                    }
                                }

                                div { class: "simple-actions",
                                    button {
                                        onclick: move |_| on_save_config.call(()),
                                        "Save Settings"
                                    }
                                }
                            }
                        }
                        if !settings_error.read().is_empty() {
                            div { class: "error", "{settings_error}" }
                        }
                    }
                }
            }
            if *active_tab.read() == UiTab::Activity {
                div { class: "settings",
                    div { class: "settings-card",
                        label { "Activity" }
                        p { class: "hint", "Background reminders, tool events, and autonomy updates appear here." }
                    }
                    div { class: "chat", id: "activity-scroll",
                        for message in activity_messages
                            .read()
                            .iter()
                            .filter(|msg| !msg.text.is_empty())
                        {
                            div {
                                class: "bubble bot",
                                div {
                                    class: "bubble-content",
                                    dangerous_inner_html: message.html.clone(),
                                }
                                div { class: "bubble-time", "{format_local_time(message.timestamp)}" }
                            }
                        }
                        if activity_messages.read().is_empty() {
                            div { class: "hint", "No activity yet." }
                        }
                    }
                }
            }
            if *active_tab.read() == UiTab::Context {
                div { class: "settings",
                    div { class: "settings-card",
                        label { "Context (Markdown)" }
                        p { class: "hint", "Source: {context_path}" }
                        div { class: "config-head",
                            label { "Editor" }
                            label { "Preview" }
                        }
                        div { class: "config-grid",
                            div { class: "config-panel",
                                textarea {
                                    id: "context-md",
                                    value: "{context_text}",
                                    rows: "18",
                                    class: "config-editor",
                                    oninput: move |evt| {
                                        let mut context_text = context_text.clone();
                                        context_text.set(evt.value());
                                    },
                                }
                            }
                            div { class: "config-panel",
                                div {
                                    class: "config-preview",
                                    dangerous_inner_html: "{markdown_to_html(&context_text.read())}",
                                }
                            }
                        }
                        div { class: "config-actions",
                            button { onclick: move |_| on_validate_context.call(()), "Validate" }
                            button {
                                disabled: is_url_source(&context_path.read()),
                                onclick: move |_| on_save_context.call(()),
                                "Save Context"
                            }
                        }
                        if is_url_source(&context_path.read()) {
                            p { class: "hint", "Remote URL sources are read-only." }
                        }
                    }
                    if !context_error.read().is_empty() {
                        div { class: "error", "{context_error}" }
                    }
                }
            }
            if *active_tab.read() == UiTab::Heartbeat {
                div { class: "settings",
                    div { class: "settings-card",
                        label { "Heartbeat (Markdown)" }
                        p { class: "hint", "Source: {heartbeat_path}" }
                        div { class: "config-head",
                            label { "Editor" }
                            label { "Preview" }
                        }
                        div { class: "config-grid",
                            div { class: "config-panel",
                                textarea {
                                    id: "heartbeat-md",
                                    value: "{heartbeat_text}",
                                    rows: "18",
                                    class: "config-editor",
                                    oninput: move |evt| {
                                        let mut heartbeat_text = heartbeat_text.clone();
                                        heartbeat_text.set(evt.value());
                                    },
                                }
                            }
                            div { class: "config-panel",
                                div {
                                    class: "config-preview",
                                    dangerous_inner_html: "{markdown_to_html(&heartbeat_text.read())}",
                                }
                            }
                        }
                        div { class: "config-actions",
                            button { onclick: move |_| on_validate_heartbeat.call(()), "Validate" }
                            button {
                                disabled: is_url_source(&heartbeat_path.read()),
                                onclick: move |_| on_save_heartbeat.call(()),
                                "Save Heartbeat"
                            }
                        }
                        if is_url_source(&heartbeat_path.read()) {
                            p { class: "hint", "Remote URL sources are read-only." }
                        }
                    }
                    if !heartbeat_error.read().is_empty() {
                        div { class: "error", "{heartbeat_error}" }
                    }
                }
            }
        }
    }
}
