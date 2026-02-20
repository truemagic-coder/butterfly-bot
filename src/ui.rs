#![allow(
    clippy::clone_on_copy,
    clippy::collapsible_match,
    clippy::collapsible_else_if
)]

use dioxus::document::eval;
use dioxus::launch;
use dioxus::prelude::*;
use futures::StreamExt;
#[cfg(target_os = "linux")]
use notify_rust::Notification;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration as StdDuration;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::html::styled_line_to_highlighted_html;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use time::format_description::well_known::Rfc3339;
use time::{macros::format_description, OffsetDateTime, UtcOffset};
use tokio::time::{sleep, timeout, Duration};

const APP_LOGO: Asset = asset!("/assets/icons/hicolor/32x32/apps/butterfly-bot.png");
const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const OPENAI_DEFAULT_CHAT_MODEL: &str = "gpt-4.1-mini";
const OPENAI_DEFAULT_EMBED_MODEL: &str = "text-embedding-3-small";
const OPENAI_DEFAULT_RERANK_MODEL: &str = "gpt-4.1-mini";

#[cfg(target_os = "linux")]
fn send_desktop_notification(title: &str) {
    if let Err(err) = Notification::new()
        .summary("Butterfly Bot")
        .body(title)
        .show()
    {
        tracing::warn!(error = %err, "Desktop notification failed");
    }
}

#[cfg(not(target_os = "linux"))]
fn send_desktop_notification(_title: &str) {}

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
    headers: HashMap<String, String>,
    primary_header_key: String,
}

#[derive(Clone, Default)]
struct UiHttpCallServer {
    name: String,
    url: String,
    header_key: String,
    header_value: String,
    headers: HashMap<String, String>,
    primary_header_key: String,
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
    launch_ui_with_config(UiLaunchConfig::default());
}

#[derive(Clone)]
pub struct UiLaunchConfig {
    pub db_path: String,
    pub daemon_url: String,
    pub user_id: String,
}

impl Default for UiLaunchConfig {
    fn default() -> Self {
        Self {
            db_path: crate::runtime_paths::default_db_path(),
            daemon_url: "http://127.0.0.1:7878".to_string(),
            user_id: "user".to_string(),
        }
    }
}

fn ui_launch_config_lock() -> &'static Mutex<UiLaunchConfig> {
    static CONFIG: OnceLock<Mutex<UiLaunchConfig>> = OnceLock::new();
    CONFIG.get_or_init(|| Mutex::new(UiLaunchConfig::default()))
}

fn set_ui_launch_config(config: UiLaunchConfig) {
    let lock = ui_launch_config_lock();
    match lock.lock() {
        Ok(mut guard) => *guard = config,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = config;
        }
    }
}

fn ui_launch_config() -> UiLaunchConfig {
    let lock = ui_launch_config_lock();
    match lock.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

pub fn launch_ui_with_config(config: UiLaunchConfig) {
    set_ui_launch_config(config);
    force_dbusrs();
    install_shutdown_hooks_once();
    launch(app_view);
    shutdown_spawned_daemon_best_effort();
}

fn shutdown_spawned_daemon_best_effort() {
    let _ = stop_local_daemon();
}

fn install_shutdown_hooks_once() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            shutdown_spawned_daemon_best_effort();
            previous(info);
        }));
    });
}

fn stream_timeout_duration() -> Duration {
    Duration::from_secs(180)
}

#[cfg(target_os = "linux")]
fn force_dbusrs() {}

#[cfg(not(target_os = "linux"))]
fn force_dbusrs() {}

struct DaemonControl {
    child: Child,
}

fn daemon_control() -> &'static Mutex<Option<DaemonControl>> {
    static CONTROL: OnceLock<Mutex<Option<DaemonControl>>> = OnceLock::new();
    CONTROL.get_or_init(|| Mutex::new(None))
}

fn local_daemon_exit_status() -> Option<std::process::ExitStatus> {
    let control = daemon_control();
    let mut guard = control.lock().ok()?;
    let control = guard.as_mut()?;
    control.child.try_wait().ok().flatten()
}

fn daemon_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    let mut push_candidate = |path: PathBuf| {
        if !candidates.iter().any(|existing| existing == &path) {
            candidates.push(path);
        }
    };

    if let Ok(explicit) = std::env::var("BUTTERFLY_BOTD_PATH") {
        let explicit = explicit.trim();
        if !explicit.is_empty() {
            push_candidate(PathBuf::from(explicit));
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            push_candidate(dir.join("butterfly-botd"));
            #[cfg(windows)]
            push_candidate(dir.join("butterfly-botd.exe"));

            if let Some(profile_dir) = dir.file_name().and_then(|value| value.to_str()) {
                if (profile_dir == "release" || profile_dir == "debug")
                    && dir.parent().is_some()
                {
                    let target_dir = dir.parent().unwrap();
                    let other_profile = if profile_dir == "release" {
                        "debug"
                    } else {
                        "release"
                    };
                    push_candidate(target_dir.join(other_profile).join("butterfly-botd"));
                    #[cfg(windows)]
                    push_candidate(target_dir.join(other_profile).join("butterfly-botd.exe"));
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        push_candidate(cwd.join("target").join("debug").join("butterfly-botd"));
        push_candidate(cwd.join("target").join("release").join("butterfly-botd"));
        #[cfg(windows)]
        {
            push_candidate(cwd.join("target").join("debug").join("butterfly-botd.exe"));
            push_candidate(cwd.join("target").join("release").join("butterfly-botd.exe"));
        }
    }

    // Fall back to PATH lookup last so local workspace/current-exe builds win.
    push_candidate(PathBuf::from("butterfly-botd"));

    candidates
}

fn daemon_log_path() -> PathBuf {
    crate::runtime_paths::app_root()
        .join("logs")
        .join("ui-daemon.log")
}

fn daemon_tpm_mode(db_path: &str) -> String {
    crate::config::Config::from_store(db_path)
        .ok()
        .and_then(|config| config.tools)
        .and_then(|tools| tools.get("settings").cloned())
        .and_then(|settings| settings.get("security").cloned())
        .and_then(|security| security.get("tpm_mode").cloned())
        .and_then(|mode| mode.as_str().map(|s| s.to_string()))
        .filter(|mode| matches!(mode.as_str(), "strict" | "auto" | "compatible"))
        .unwrap_or_else(|| "auto".to_string())
}

fn spawn_daemon_process(
    host: String,
    port: u16,
    db_path: String,
    token: String,
    tpm_mode: String,
) -> Result<Child, String> {
    let candidates = daemon_binary_candidates();
    let mut not_found = Vec::new();
    let log_path = daemon_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create daemon log directory '{}': {err}",
                parent.to_string_lossy()
            )
        })?;
    }

    for candidate in candidates {
        let candidate_display = candidate.to_string_lossy().to_string();
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|err| {
                format!(
                    "Failed to open daemon log file '{}': {err}",
                    log_path.to_string_lossy()
                )
            })?;
        let stdout_log = log_file.try_clone().map_err(|err| {
            format!(
                "Failed to clone daemon log file handle '{}': {err}",
                log_path.to_string_lossy()
            )
        })?;
        let result = Command::new(&candidate)
            .arg("--host")
            .arg(host.clone())
            .arg("--port")
            .arg(port.to_string())
            .arg("--db")
            .arg(db_path.clone())
            .env("BUTTERFLY_BOT_TOKEN", token.clone())
            .env("BUTTERFLY_TPM_MODE", tpm_mode.clone())
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(log_file))
            .spawn();

        match result {
            Ok(child) => return Ok(child),
            Err(err) if err.kind() == ErrorKind::NotFound => {
                not_found.push(candidate_display);
            }
            Err(err) => {
                return Err(format!(
                    "Failed to start butterfly-botd process via '{}': {err} (log: {})",
                    candidate_display,
                    log_path.to_string_lossy()
                ));
            }
        }
    }

    Err(format!(
        "Failed to start butterfly-botd process: executable not found. Tried: {}. Set BUTTERFLY_BOTD_PATH or ensure butterfly-botd is installed.",
        not_found.join(", ")
    ))
}

fn start_local_daemon() -> Result<(), String> {
    let control = daemon_control();
    let mut guard = control
        .lock()
        .map_err(|_| "Daemon lock unavailable".to_string())?;
    if let Some(control) = guard.as_mut() {
        match control.child.try_wait() {
            Ok(Some(_)) => {
                *guard = None;
            }
            Ok(None) => return Ok(()),
            Err(_) => {
                *guard = None;
            }
        }
    }

    let config = ui_launch_config();
    let daemon_url = config.daemon_url;
    let (host, port) = parse_daemon_address(&daemon_url);
    let daemon_addr = format!("{host}:{port}");
    let db_path = config.db_path;
    let token = env_auth_token();
    let tpm_mode = daemon_tpm_mode(&db_path);

    let mut child = spawn_daemon_process(host.clone(), port, db_path, token, tpm_mode)?;

    // Detect immediate exit (e.g. bind conflict) so UI doesn't talk to a different
    // daemon instance with a mismatched auth token and then fail with HTTP 401.
    std::thread::sleep(StdDuration::from_millis(200));
    if let Ok(Some(status)) = child.try_wait() {
        if daemon_health_ok(&host, port) {
            tracing::info!(address = %daemon_addr, "Daemon already healthy on target address; reusing existing instance");
            return Ok(());
        }
        return Err(format!(
            "Daemon exited immediately ({status}) while starting on {daemon_addr}. See log: {}",
            daemon_log_path().to_string_lossy()
        ));
    }

    tracing::info!("UI spawned local daemon process");

    *guard = Some(DaemonControl { child });

    Ok(())
}

fn daemon_health_ok(host: &str, port: u16) -> bool {
    let address = format!("{host}:{port}");
    let socket = match address.to_socket_addrs().ok().and_then(|mut addrs| addrs.next()) {
        Some(value) => value,
        None => return false,
    };

    let mut stream = match std::net::TcpStream::connect_timeout(&socket, StdDuration::from_millis(500)) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let _ = stream.set_write_timeout(Some(StdDuration::from_millis(500)));
    let _ = stream.set_read_timeout(Some(StdDuration::from_millis(700)));

    let request = format!(
        "GET /health HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = [0u8; 256];
    let bytes_read = match stream.read(&mut response) {
        Ok(count) if count > 0 => count,
        _ => return false,
    };
    let text = String::from_utf8_lossy(&response[..bytes_read]);
    text.contains(" 200 ") || text.contains("{\"status\":\"ok\"}")
}

fn env_auth_token() -> String {
    crate::vault::ensure_daemon_auth_token().unwrap_or_default()
}

fn stop_local_daemon() -> Result<(), String> {
    let control = daemon_control();
    let mut guard = control
        .lock()
        .map_err(|_| "Daemon lock unavailable".to_string())?;
    if let Some(mut control) = guard.take() {
        if let Ok(Some(_)) = control.child.try_wait() {
            tracing::info!("Local daemon already exited");
            return Ok(());
        }
        let _ = control.child.kill();
        tracing::info!("Sent kill signal to local daemon");

        // Avoid long blocking waits that can make desktop shutdown appear hung.
        for _ in 0..80 {
            match control.child.try_wait() {
                Ok(Some(_)) => {
                    tracing::info!("Local daemon exited");
                    return Ok(());
                }
                Ok(None) => std::thread::sleep(StdDuration::from_millis(25)),
                Err(err) => {
                    tracing::error!(error = %err, "Failed waiting for local daemon shutdown");
                    return Err(format!("Failed waiting for daemon shutdown: {err}"));
                }
            }
        }

        tracing::warn!("Local daemon did not exit before shutdown timeout");

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
    let launch_config = ui_launch_config();
    let db_path = launch_config.db_path;
    let daemon_url = use_signal(|| normalize_daemon_url(&launch_config.daemon_url));
    let token = use_signal(env_auth_token);
    let user_id = use_signal(|| launch_config.user_id.clone());
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
    let tpm_mode_input = use_signal(|| "auto".to_string());
    let openai_api_key_input = use_signal(String::new);
    let openai_base_url_input = use_signal(String::new);
    let openai_model_input = use_signal(String::new);
    let openai_embedding_model_input = use_signal(String::new);
    let openai_rerank_model_input = use_signal(String::new);
    let coding_api_key_input = use_signal(String::new);
    let search_api_key_input = use_signal(String::new);
    let solana_rpc_endpoint_input = use_signal(String::new);
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
    let openai_settings_expanded = use_signal(|| false);

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
                let mut token = token;
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

                        // Wait for daemon to be ready (retry up to 60 times with 500ms delay)
                        // Startup can take a while on first run while security checks, WASM
                        // provisioning, embeddings, and prompt/context bootstrapping complete.
                        let client = reqwest::Client::new();
                        let mut daemon_ready = false;
                        let mut daemon_exit: Option<std::process::ExitStatus> = None;
                        for i in 0..60 {
                            sleep(Duration::from_millis(500)).await;
                            if let Some(status) = local_daemon_exit_status() {
                                daemon_exit = Some(status);
                                break;
                            }
                            let health_url =
                                format!("{}/health", daemon_url().trim_end_matches('/'));
                            if let Ok(resp) = client.get(&health_url).send().await {
                                if resp.status().is_success() {
                                    daemon_ready = true;
                                    break;
                                }
                            }
                            boot_status.set(format!("Waiting for daemon... ({}/60)", i + 1));
                        }

                        if !daemon_ready {
                            if let Some(status) = daemon_exit {
                                daemon_running.set(false);
                                boot_ready.set(false);
                                daemon_status.set(format!(
                                    "Daemon exited during startup: {status}."
                                ));
                                boot_status.set(
                                    "Daemon failed to start. Check daemon logs/token and retry."
                                        .to_string(),
                                );
                            } else {
                                daemon_running.set(false);
                                boot_ready.set(false);
                                daemon_status.set(
                                    "Daemon did not become healthy within startup timeout."
                                        .to_string(),
                                );
                                boot_status.set(
                                    "Daemon startup timed out. Retry Start; ensure no other daemon is bound to this port."
                                        .to_string(),
                                );
                            }
                        } else {
                            daemon_running.set(true);
                            let url =
                                format!("{}/preload_boot", daemon_url().trim_end_matches('/'));
                            let preload_body = PreloadBootRequest { user_id: user_id() };
                            let mut token_value = token();
                            if token_value.trim().is_empty() {
                                let refreshed = env_auth_token();
                                if !refreshed.trim().is_empty() {
                                    token.set(refreshed.clone());
                                    token_value = refreshed;
                                }
                            }

                            let send_preload = |token_value: &str| {
                                let mut request = client.post(&url).json(&preload_body);
                                if !token_value.trim().is_empty() {
                                    request = request
                                        .header("authorization", format!("Bearer {token_value}"));
                                }
                                request
                            };

                            match send_preload(&token_value).send().await {
                                Ok(resp) if resp.status().is_success() => {
                                    boot_status
                                        .set("Boot preload started in background…".to_string());
                                }
                                Ok(resp) => {
                                    let status = resp.status();
                                    if status == reqwest::StatusCode::UNAUTHORIZED {
                                        let refreshed = env_auth_token();
                                        if !refreshed.trim().is_empty() && refreshed != token_value {
                                            token.set(refreshed.clone());
                                            token_value = refreshed;
                                            match send_preload(&token_value).send().await {
                                                Ok(retry_resp) if retry_resp.status().is_success() => {
                                                    boot_status.set(
                                                        "Boot preload started in background…"
                                                            .to_string(),
                                                    );
                                                }
                                                Ok(retry_resp)
                                                    if retry_resp.status()
                                                        == reqwest::StatusCode::UNAUTHORIZED =>
                                                {
                                                    boot_status.set(
                                                        "Boot preload failed: HTTP 401 Unauthorized. Likely daemon auth token mismatch. Stop other daemon instances and start daemon from this UI session."
                                                            .to_string(),
                                                    );
                                                    daemon_status.set(
                                                        "Daemon auth mismatch detected (401). Ensure one daemon instance and shared BUTTERFLY_BOT_TOKEN."
                                                            .to_string(),
                                                    );
                                                }
                                                Ok(retry_resp) => {
                                                    let retry_status = retry_resp.status();
                                                    boot_status.set(format!(
                                                        "Boot preload failed: HTTP {retry_status}"
                                                    ));
                                                }
                                                Err(err) => {
                                                    boot_status
                                                        .set(format!("Boot preload error: {err}"));
                                                }
                                            }
                                        } else {
                                            boot_status.set(
                                                "Boot preload failed: HTTP 401 Unauthorized. Likely daemon auth token mismatch. Stop other daemon instances and start daemon from this UI session."
                                                    .to_string(),
                                            );
                                            daemon_status.set(
                                                "Daemon auth mismatch detected (401). Ensure one daemon instance and shared BUTTERFLY_BOT_TOKEN."
                                                    .to_string(),
                                            );
                                        }
                                    } else {
                                        boot_status
                                            .set(format!("Boot preload failed: HTTP {status}"));
                                    }
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

    let on_run_doctor = {
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let doctor_status = doctor_status.clone();
        let doctor_error = doctor_error.clone();
        let doctor_running = doctor_running.clone();
        let doctor_overall = doctor_overall.clone();
        let doctor_checks = doctor_checks.clone();

        use_callback(move |_: ()| {
            let daemon_running = daemon_running.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();
            let doctor_status = doctor_status.clone();
            let doctor_error = doctor_error.clone();
            let doctor_running = doctor_running.clone();
            let doctor_overall = doctor_overall.clone();
            let doctor_checks = doctor_checks.clone();

            spawn(async move {
                let mut doctor_status = doctor_status;
                let mut doctor_error = doctor_error;
                let mut doctor_running = doctor_running;
                let mut doctor_overall = doctor_overall;
                let mut doctor_checks = doctor_checks;

                if !daemon_running() {
                    doctor_error
                        .set("Doctor requires a running daemon. Start daemon first.".to_string());
                    doctor_status.set(String::new());
                    return;
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
                        doctor_overall.set(String::new());
                        doctor_checks.set(Vec::new());
                    }
                }

                doctor_running.set(false);
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
        let next_id = next_id.clone();

        use_effect(move || {
            if *history_load_started.read() || !*daemon_running.read() {
                return;
            }

            let mut started = history_load_started.clone();
            started.set(true);

            let mut messages = messages.clone();
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
                            if cfg!(debug_assertions) {
                                tracing::debug!("Reminder stream request failed (daemon unreachable?)");
                            }
                            sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    if !response.status().is_success() {
                        if cfg!(debug_assertions) {
                            tracing::debug!(status = %response.status(), "Reminder stream returned non-success status");
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
                                        send_desktop_notification(title);
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

                                        let show_success = false;
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
        let tpm_mode_input = tpm_mode_input.clone();
        let openai_api_key_input = openai_api_key_input.clone();
        let openai_base_url_input = openai_base_url_input.clone();
        let openai_model_input = openai_model_input.clone();
        let openai_embedding_model_input = openai_embedding_model_input.clone();
        let openai_rerank_model_input = openai_rerank_model_input.clone();
        let coding_api_key_input = coding_api_key_input.clone();
        let search_api_key_input = search_api_key_input.clone();
        let solana_rpc_endpoint_input = solana_rpc_endpoint_input.clone();
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
                let mut tpm_mode_input = tpm_mode_input;
                let mut openai_api_key_input = openai_api_key_input;
                let mut openai_base_url_input = openai_base_url_input;
                let mut openai_model_input = openai_model_input;
                let mut openai_embedding_model_input = openai_embedding_model_input;
                let mut openai_rerank_model_input = openai_rerank_model_input;
                let mut coding_api_key_input = coding_api_key_input;
                let mut search_api_key_input = search_api_key_input;
                let mut solana_rpc_endpoint_input = solana_rpc_endpoint_input;
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
                        tools_loaded.set(true);
                        return;
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

                let tpm_mode = config
                    .tools
                    .as_ref()
                    .and_then(|tools| tools.get("settings"))
                    .and_then(|settings| settings.get("security"))
                    .and_then(|security| security.get("tpm_mode"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("auto");
                tpm_mode_input.set(match tpm_mode {
                    "strict" | "auto" | "compatible" => tpm_mode.to_string(),
                    _ => "auto".to_string(),
                });

                if let Some(openai) = &config.openai {
                    if let Some(base_url) = &openai.base_url {
                        openai_base_url_input.set(base_url.clone());
                    }
                    if let Some(model) = &openai.model {
                        openai_model_input.set(model.clone());
                    }
                }

                if let Some(memory) = &config.memory {
                    if let Some(embedding_model) = &memory.embedding_model {
                        openai_embedding_model_input.set(embedding_model.clone());
                    }
                    if let Some(rerank_model) = &memory.rerank_model {
                        openai_rerank_model_input.set(rerank_model.clone());
                    }
                }

                if let Some(tools_value) = &config.tools {
                    if let Some(wakeup_cfg) = tools_value.get("wakeup") {
                        if let Some(poll_seconds) =
                            wakeup_cfg.get("poll_seconds").and_then(|v| v.as_u64())
                        {
                            wakeup_poll_seconds_input.set(poll_seconds.to_string());
                        }
                    }

                    if let Some(solana_rpc_endpoint) = tools_value
                        .get("settings")
                        .and_then(|settings| settings.get("solana"))
                        .and_then(|solana| solana.get("rpc"))
                        .and_then(|rpc| rpc.get("endpoint"))
                        .and_then(|value| value.as_str())
                    {
                        solana_rpc_endpoint_input.set(solana_rpc_endpoint.to_string());
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
                                let headers = entry
                                    .get("headers")
                                    .and_then(|v| v.as_object())
                                    .map(|map| {
                                        map.iter()
                                            .filter_map(|(k, v)| {
                                                v.as_str().map(|value| {
                                                    (k.trim().to_string(), value.trim().to_string())
                                                })
                                            })
                                            .collect::<HashMap<_, _>>()
                                    })
                                    .unwrap_or_default();
                                let mut header_entries = headers.iter().collect::<Vec<_>>();
                                header_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
                                let (header_key, header_value) = header_entries
                                    .first()
                                    .map(|(key, value)| ((*key).clone(), (*value).clone()))
                                    .unwrap_or_else(|| (String::new(), String::new()));
                                if name.is_empty() && url.is_empty() {
                                    None
                                } else {
                                    let primary_header_key = header_key.clone();
                                    Some(UiMcpServer {
                                        name,
                                        url,
                                        header_key,
                                        header_value,
                                        headers,
                                        primary_header_key,
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
                                        let headers = entry
                                            .get("headers")
                                            .and_then(|v| v.as_object())
                                            .map(|map| {
                                                map.iter()
                                                    .filter_map(|(k, v)| {
                                                        v.as_str().map(|value| {
                                                            (
                                                                k.trim().to_string(),
                                                                value.trim().to_string(),
                                                            )
                                                        })
                                                    })
                                                    .collect::<HashMap<_, _>>()
                                            })
                                            .unwrap_or_default();
                                        let mut header_entries = headers.iter().collect::<Vec<_>>();
                                        header_entries
                                            .sort_by(|(left, _), (right, _)| left.cmp(right));
                                        let (header_key, header_value) = header_entries
                                            .first()
                                            .map(|(key, value)| ((*key).clone(), (*value).clone()))
                                            .unwrap_or_else(|| (String::new(), String::new()));

                                        if name.is_empty() && url.is_empty() {
                                            None
                                        } else {
                                            let primary_header_key = header_key.clone();
                                            Some(UiHttpCallServer {
                                                name,
                                                url,
                                                header_key,
                                                header_value,
                                                headers,
                                                primary_header_key,
                                            })
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        if parsed_servers.is_empty() {
                            let mut shared_headers = http_call_cfg
                                .get("custom_headers")
                                .and_then(|v| v.as_object())
                                .map(|map| {
                                    map.iter()
                                        .filter_map(|(k, v)| {
                                            v.as_str().map(|value| {
                                                (k.trim().to_string(), value.trim().to_string())
                                            })
                                        })
                                        .collect::<HashMap<_, _>>()
                                })
                                .unwrap_or_default();
                            if let Some(default_headers) = http_call_cfg
                                .get("default_headers")
                                .and_then(|v| v.as_object())
                            {
                                for (key, value) in default_headers {
                                    if let Some(value) = value.as_str() {
                                        shared_headers
                                            .entry(key.trim().to_string())
                                            .or_insert(value.trim().to_string());
                                    }
                                }
                            }
                            let mut header_entries = shared_headers.iter().collect::<Vec<_>>();
                            header_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
                            let (header_key, header_value) = header_entries
                                .first()
                                .map(|(key, value)| ((*key).clone(), (*value).clone()))
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
                                            let primary_header_key = header_key.clone();
                                            Some(UiHttpCallServer {
                                                name: format!("server_{}", index + 1),
                                                url,
                                                header_key: header_key.clone(),
                                                header_value: header_value.clone(),
                                                headers: shared_headers.clone(),
                                                primary_header_key,
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
                                    let primary_header_key = header_key.clone();
                                    parsed_servers.push(UiHttpCallServer {
                                        name: "default".to_string(),
                                        url: base_url,
                                        header_key,
                                        header_value,
                                        headers: shared_headers,
                                        primary_header_key,
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
                    Err(err) => {
                        settings_error.set(format!("Vault error: {err}"));
                        tools_loaded.set(true);
                        return;
                    }
                }

                match crate::vault::get_secret("zapier_token") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        zapier_token_input.set(secret);
                    }
                    Ok(_) => zapier_token_input.set(String::new()),
                    Err(err) => {
                        settings_error.set(format!("Vault error: {err}"));
                        tools_loaded.set(true);
                        return;
                    }
                }

                match crate::vault::get_secret("openai_api_key") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        openai_api_key_input.set(secret);
                    }
                    Ok(_) => {
                        let fallback = config
                            .openai
                            .as_ref()
                            .and_then(|openai| openai.api_key.clone())
                            .unwrap_or_default();
                        openai_api_key_input.set(fallback);
                    }
                    Err(err) => {
                        settings_error.set(format!("Vault error: {err}"));
                        tools_loaded.set(true);
                        return;
                    }
                }

                match crate::vault::get_secret("coding_openai_api_key") {
                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                        coding_api_key_input.set(secret);
                    }
                    Ok(_) => coding_api_key_input.set(String::new()),
                    Err(err) => {
                        settings_error.set(format!("Vault error: {err}"));
                        tools_loaded.set(true);
                        return;
                    }
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
                        settings_error.set(format!("Vault error: {err}"));
                        tools_loaded.set(true);
                        return;
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
        let tpm_mode_input = tpm_mode_input.clone();
        let openai_api_key_input = openai_api_key_input.clone();
        let openai_base_url_input = openai_base_url_input.clone();
        let openai_model_input = openai_model_input.clone();
        let openai_embedding_model_input = openai_embedding_model_input.clone();
        let openai_rerank_model_input = openai_rerank_model_input.clone();
        let coding_api_key_input = coding_api_key_input.clone();
        let solana_rpc_endpoint_input = solana_rpc_endpoint_input.clone();
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
            let tpm_mode_input = tpm_mode_input.clone();
            let openai_api_key_input = openai_api_key_input.clone();
            let openai_base_url_input = openai_base_url_input.clone();
            let openai_model_input = openai_model_input.clone();
            let openai_embedding_model_input = openai_embedding_model_input.clone();
            let openai_rerank_model_input = openai_rerank_model_input.clone();
            let coding_api_key_input = coding_api_key_input.clone();
            let solana_rpc_endpoint_input = solana_rpc_endpoint_input.clone();
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

                let tpm_mode_value = match tpm_mode_input().trim() {
                    "strict" | "auto" | "compatible" => tpm_mode_input().trim().to_string(),
                    _ => {
                        settings_error
                            .set("TPM mode must be strict, auto, or compatible.".to_string());
                        return;
                    }
                };

                let mut openai_base_url_value = openai_base_url_input().trim().to_string();
                let mut openai_model_value = openai_model_input().trim().to_string();
                let mut openai_embedding_model_value =
                    openai_embedding_model_input().trim().to_string();
                let mut openai_rerank_model_value = openai_rerank_model_input().trim().to_string();

                let runtime_provider_value = if openai_base_url_value
                    .to_ascii_lowercase()
                    .starts_with("http://localhost:11434")
                    || openai_base_url_value
                        .to_ascii_lowercase()
                        .starts_with("http://127.0.0.1:11434")
                {
                    "ollama"
                } else {
                    "openai"
                };

                if runtime_provider_value == "openai" {
                    if openai_base_url_value.is_empty() {
                        openai_base_url_value = OPENAI_DEFAULT_BASE_URL.to_string();
                    }
                    if openai_model_value.is_empty() {
                        openai_model_value = OPENAI_DEFAULT_CHAT_MODEL.to_string();
                    }
                    if openai_embedding_model_value.is_empty() {
                        openai_embedding_model_value = OPENAI_DEFAULT_EMBED_MODEL.to_string();
                    }
                    if openai_rerank_model_value.is_empty() {
                        openai_rerank_model_value = OPENAI_DEFAULT_RERANK_MODEL.to_string();
                    }
                }

                let mut mcp_servers = Vec::new();
                for entry in mcp_servers_form().iter() {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    let primary_header_key = entry.primary_header_key.trim();
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
                    let mut headers = entry.headers.clone();
                    if !primary_header_key.is_empty() && primary_header_key != header_key {
                        headers.remove(primary_header_key);
                    }
                    if !header_key.is_empty() {
                        headers.insert(header_key.to_string(), header_value.to_string());
                    }
                    mcp_servers.push((name.to_string(), url.to_string(), headers));
                }

                let mut http_call_servers = Vec::new();
                for entry in http_call_servers_form().iter() {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    let primary_header_key = entry.primary_header_key.trim();
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

                    let mut headers = entry.headers.clone();
                    if !primary_header_key.is_empty() && primary_header_key != header_key {
                        headers.remove(primary_header_key);
                    }
                    if !header_key.is_empty() {
                        headers.insert(header_key.to_string(), header_value.to_string());
                    }

                    http_call_servers.push((name.to_string(), url.to_string(), headers));
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

                let solana_rpc_endpoint = solana_rpc_endpoint_input().trim().to_string();

                let mut config = match crate::config::Config::from_store(&db_path) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Failed to load current config: {err}"));
                        return;
                    }
                };

                config.provider = None;

                let openai_cfg = config.openai.get_or_insert(crate::config::OpenAiConfig {
                    api_key: None,
                    model: None,
                    base_url: None,
                });
                openai_cfg.base_url = if openai_base_url_value.is_empty() {
                    None
                } else {
                    Some(openai_base_url_value)
                };
                openai_cfg.model = if openai_model_value.is_empty() {
                    None
                } else {
                    Some(openai_model_value)
                };
                openai_cfg.api_key = None;

                let memory_cfg = config.memory.get_or_insert(crate::config::MemoryConfig {
                    enabled: Some(true),
                    sqlite_path: Some(db_path.clone()),
                    summary_model: None,
                    embedding_model: None,
                    rerank_model: None,
                    openai: None,
                    context_embed_enabled: Some(false),
                    summary_threshold: None,
                    retention_days: None,
                });
                memory_cfg.embedding_model = if openai_embedding_model_value.is_empty() {
                    None
                } else {
                    Some(openai_embedding_model_value)
                };
                memory_cfg.rerank_model = if openai_rerank_model_value.is_empty() {
                    None
                } else {
                    Some(openai_rerank_model_value)
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
                                .map(|(name, url, headers)| {
                                    let mut server = serde_json::Map::new();
                                    server.insert("name".to_string(), Value::String(name));
                                    server.insert("url".to_string(), Value::String(url));
                                    if !headers.is_empty() {
                                        let headers = headers
                                            .into_iter()
                                            .map(|(key, value)| (key, Value::String(value)))
                                            .collect::<serde_json::Map<_, _>>();
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
                    let mut first_headers: Option<HashMap<String, String>> = None;

                    let servers = http_call_servers
                        .iter()
                        .map(|(name, url, headers)| {
                            if first_url.is_none() {
                                first_url = Some(url.clone());
                            }
                            if first_headers.is_none() {
                                first_headers = Some(headers.clone());
                            }
                            let mut server = serde_json::Map::new();
                            server.insert("name".to_string(), Value::String(name.clone()));
                            server.insert("url".to_string(), Value::String(url.clone()));
                            if !headers.is_empty() {
                                let headers = headers
                                    .iter()
                                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                                    .collect::<serde_json::Map<_, _>>();
                                server.insert("headers".to_string(), Value::Object(headers));
                            }
                            Value::Object(server)
                        })
                        .collect::<Vec<_>>();

                    http_call_obj.insert("servers".to_string(), Value::Array(servers));

                    let base_urls = http_call_servers
                        .iter()
                        .map(|(_, url, _)| Value::String(url.clone()))
                        .collect::<Vec<_>>();
                    http_call_obj.insert("base_urls".to_string(), Value::Array(base_urls));

                    if let Some(url) = first_url {
                        http_call_obj.insert("base_url".to_string(), Value::String(url));
                    } else {
                        http_call_obj.remove("base_url");
                    }

                    let legacy_headers = first_headers
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(key, value)| (key, Value::String(value)))
                        .collect::<serde_json::Map<_, _>>();
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

                    let security_cfg = settings_obj
                        .entry("security")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if !security_cfg.is_object() {
                        *security_cfg = Value::Object(serde_json::Map::new());
                    }
                    if let Some(security_obj) = security_cfg.as_object_mut() {
                        security_obj.insert(
                            "tpm_mode".to_string(),
                            Value::String(tpm_mode_value.clone()),
                        );
                    }

                    let solana_cfg = settings_obj
                        .entry("solana")
                        .or_insert_with(|| Value::Object(serde_json::Map::new()));
                    if !solana_cfg.is_object() {
                        *solana_cfg = Value::Object(serde_json::Map::new());
                    }
                    if let Some(solana_obj) = solana_cfg.as_object_mut() {
                        let rpc_cfg = solana_obj
                            .entry("rpc")
                            .or_insert_with(|| Value::Object(serde_json::Map::new()));
                        if !rpc_cfg.is_object() {
                            *rpc_cfg = Value::Object(serde_json::Map::new());
                        }
                        if let Some(rpc_obj) = rpc_cfg.as_object_mut() {
                            rpc_obj
                                .insert("endpoint".to_string(), Value::String(solana_rpc_endpoint));
                        }
                    }
                }

                let github_pat = github_pat_input().trim().to_string();
                std::env::set_var("BUTTERFLY_TPM_MODE", tpm_mode_value);
                if let Err(err) = crate::vault::set_secret("github_pat", &github_pat) {
                    settings_error.set(format!("Failed to store GitHub token: {err}"));
                    return;
                }

                let zapier_token = zapier_token_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret("zapier_token", &zapier_token) {
                    settings_error.set(format!("Failed to store Zapier token: {err}"));
                    return;
                }

                let openai_api_key = openai_api_key_input().trim().to_string();
                if let Err(err) = crate::vault::set_secret("openai_api_key", &openai_api_key) {
                    settings_error.set(format!("Failed to store OpenAI API key: {err}"));
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

    let on_factory_reset_config = {
        let settings_error = settings_error.clone();
        let settings_status = settings_status.clone();
        let settings_load_started = settings_load_started.clone();
        let tools_loaded = tools_loaded.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();

        use_callback(move |_: ()| {
            let settings_error = settings_error.clone();
            let settings_status = settings_status.clone();
            let settings_load_started = settings_load_started.clone();
            let tools_loaded = tools_loaded.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut settings_status = settings_status;
                let mut settings_load_started = settings_load_started;
                let mut tools_loaded = tools_loaded;

                settings_error.set(String::new());
                settings_status.set("Resetting config to defaults…".to_string());

                match run_factory_reset_config_request(daemon_url(), token()).await {
                    Ok(response) => {
                        settings_status.set(response.message);
                        tools_loaded.set(false);
                        settings_load_started.set(false);
                    }
                    Err(err) => {
                        settings_error.set(format!("Factory reset failed: {err}"));
                        settings_status.set(String::new());
                    }
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
            .nav {{
                display: flex;
                gap: 8px;
                align-items: center;
                justify-content: flex-end;
                margin-left: auto;
                flex-wrap: wrap;
            }}
            .nav > button {{
                min-width: 0;
                padding: 8px 14px;
            }}
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
            .server-row {{
                display: grid;
                grid-template-columns: minmax(140px, 1fr) minmax(220px, 2fr) minmax(160px, 1fr) minmax(180px, 1fr) auto;
                align-items: center;
                gap: 10px;
            }}
            .server-row input,
            .server-row button {{
                width: 100%;
                min-width: 0;
                box-sizing: border-box;
            }}
            .simple-row .mcp-name {{
                flex: 1;
            }}
            .simple-row .mcp-url {{
                flex: 1;
            }}
            .simple-row .mcp-header-key {{
                flex: 1;
            }}
            .simple-row .mcp-header-value {{
                flex: 1;
            }}
            .simple-row .host-input {{
                flex: 1;
            }}
            .network-row {{
                display: grid;
                grid-template-columns: minmax(0, 1fr) auto;
                align-items: center;
                gap: 10px;
            }}
            .network-row input,
            .network-row button {{
                width: 100%;
                min-width: 0;
                box-sizing: border-box;
            }}
            .network-row button {{
                min-width: 108px;
            }}
            .simple-actions {{
                display: flex;
                justify-content: center;
                gap: 14px;
                margin-top: 2px;
            }}
            .add-host-actions {{
                margin-top: 12px;
            }}
            @media (max-width: 1280px) {{
                .server-row {{
                    grid-template-columns: minmax(180px, 1fr) minmax(220px, 1fr);
                }}
                .server-row button {{
                    grid-column: 1 / -1;
                }}
            }}
            @media (max-width: 1024px) {{
                .server-row {{
                    grid-template-columns: 1fr;
                }}
                .server-row button {{
                    grid-column: auto;
                }}
                .network-row {{
                    grid-template-columns: 1fr;
                }}
            }}
            @media (max-width: 960px) {{
                .header {{
                    padding: 12px 14px;
                    justify-content: space-between;
                    gap: 8px;
                }}
                .nav {{
                    width: auto;
                    gap: 6px;
                    justify-content: flex-end;
                    margin-left: auto;
                    flex-wrap: nowrap;
                }}
                .nav > button {{
                    flex: 0 0 auto;
                }}
                .nav-controls {{
                    width: auto;
                    margin-left: auto;
                    display: flex;
                    gap: 6px;
                }}
                .daemon-icon-btn {{
                    width: 34px;
                    height: 34px;
                    min-width: 34px;
                    border-radius: 999px;
                }}
                .daemon-trash-btn {{
                    margin-left: 4px;
                }}
            }}
            @media (max-width: 640px) {{
                .header {{
                    justify-content: space-between;
                    flex-wrap: wrap;
                }}
                .nav {{
                    width: 100%;
                    justify-content: flex-end;
                    margin-left: 0;
                    flex-wrap: wrap;
                }}
                .nav > button {{
                    flex: 0 0 auto;
                }}
                .nav-controls {{
                    width: auto;
                    margin-left: 8px;
                    display: flex;
                }}
                .daemon-icon-btn {{
                    width: 34px;
                    height: 34px;
                    min-width: 34px;
                    border-radius: 999px;
                }}
                .daemon-trash-btn {{
                    margin-left: 4px;
                }}
            }}
            @media (max-width: 860px) {{
                .simple-top {{ grid-template-columns: 1fr; }}
                .simple-row {{ flex-direction: column; align-items: stretch; }}
                .server-row {{ grid-template-columns: 1fr; }}
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
                                        label { "Solana RPC Endpoint" }
                                        input {
                                            value: "{solana_rpc_endpoint_input}",
                                            oninput: move |evt| {
                                                let mut solana_rpc_endpoint_input = solana_rpc_endpoint_input.clone();
                                                solana_rpc_endpoint_input.set(evt.value());
                                            },
                                            placeholder: "https://...",
                                        }
                                    }
                                    div {
                                        label { "TPM Mode" }
                                        select {
                                            value: "{tpm_mode_input}",
                                            onchange: move |evt| {
                                                let selected = evt.value();
                                                let normalized = match selected.as_str() {
                                                    "strict" | "auto" | "compatible" => selected,
                                                    _ => "auto".to_string(),
                                                };
                                                let mut tpm_mode_input = tpm_mode_input.clone();
                                                tpm_mode_input.set(normalized);
                                            },
                                            option {
                                                value: "strict",
                                                "strict"
                                            }
                                            option {
                                                value: "auto",
                                                "auto"
                                            }
                                            option {
                                                value: "compatible",
                                                "compatible"
                                            }
                                        }
                                    }
                                    div {}
                                }

                                div { class: "simple-section",
                                    label { "OpenAI Settings" }
                                    button {
                                        class: "text-btn",
                                        onclick: move |_| {
                                            let mut openai_settings_expanded = openai_settings_expanded.clone();
                                            openai_settings_expanded.set(!openai_settings_expanded());
                                        },
                                        if openai_settings_expanded() {
                                            "Hide"
                                        } else {
                                            "Show"
                                        }
                                    }

                                    if openai_settings_expanded() {
                                        div { class: "simple-top",
                                            div {
                                                label { "OpenAI Base URL" }
                                                input {
                                                    value: "{openai_base_url_input}",
                                                    oninput: move |evt| {
                                                        let mut openai_base_url_input = openai_base_url_input.clone();
                                                        openai_base_url_input.set(evt.value());
                                                    },
                                                    placeholder: "https://api.openai.com/v1",
                                                }
                                            }
                                            div {
                                                label { "OpenAI API Key" }
                                                input {
                                                    r#type: "password",
                                                    value: "{openai_api_key_input}",
                                                    oninput: move |evt| {
                                                        let mut openai_api_key_input = openai_api_key_input.clone();
                                                        openai_api_key_input.set(evt.value());
                                                    },
                                                    placeholder: "Paste OpenAI API key",
                                                }
                                            }
                                            div {
                                                label { "Runtime Model" }
                                                input {
                                                    value: "{openai_model_input}",
                                                    oninput: move |evt| {
                                                        let mut openai_model_input = openai_model_input.clone();
                                                        openai_model_input.set(evt.value());
                                                    },
                                                    placeholder: "gpt-4.1-mini",
                                                }
                                            }
                                        }

                                        div { class: "simple-top",
                                            div {
                                                label { "Embedding Model (small)" }
                                                input {
                                                    value: "{openai_embedding_model_input}",
                                                    oninput: move |evt| {
                                                        let mut openai_embedding_model_input = openai_embedding_model_input.clone();
                                                        openai_embedding_model_input.set(evt.value());
                                                    },
                                                    placeholder: "text-embedding-3-small",
                                                }
                                            }
                                            div {
                                                label { "Rerank Model" }
                                                input {
                                                    value: "{openai_rerank_model_input}",
                                                    oninput: move |evt| {
                                                        let mut openai_rerank_model_input = openai_rerank_model_input.clone();
                                                        openai_rerank_model_input.set(evt.value());
                                                    },
                                                    placeholder: "owner-selected reranker",
                                                }
                                            }
                                            div {}
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
                                                let mut search_api_key_status = search_api_key_status.clone();
                                                match crate::vault::get_secret(secret_name) {
                                                    Ok(Some(secret)) if !secret.trim().is_empty() => {
                                                        search_api_key_input.set(secret);
                                                        search_api_key_status.set("Stored in vault".to_string());
                                                    }
                                                    Ok(_) => {
                                                        search_api_key_input.set(String::new());
                                                        search_api_key_status.set("Not set".to_string());
                                                    }
                                                    Err(err) => {
                                                        search_api_key_input.set(String::new());
                                                        search_api_key_status.set(format!("Vault error: {err}"));
                                                    }
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
                                            div { class: "simple-row server-row",
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
                                            div { class: "simple-row server-row",
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
                                            div { class: "simple-row network-row",
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

                                div { class: "simple-section",
                                    label { "Doctor" }
                                    p { class: "hint", "Run diagnostics for provider health, security posture, and daemon readiness." }
                                    div { class: "simple-actions",
                                        button {
                                            onclick: move |_| on_run_doctor.call(()),
                                            disabled: *doctor_running.read(),
                                            if *doctor_running.read() { "Running…" } else { "Run Doctor" }
                                        }
                                    }
                                    if !doctor_status.read().is_empty() {
                                        p { class: "hint", "{doctor_status}" }
                                    }
                                    if !doctor_overall.read().is_empty() {
                                        p { class: "hint", "Overall: {doctor_overall}" }
                                    }
                                    if !doctor_error.read().is_empty() {
                                        p { class: "error", "{doctor_error}" }
                                    }
                                    if !doctor_checks.read().is_empty() {
                                        div { class: "simple-list",
                                            for check in doctor_checks.read().iter() {
                                                div { class: "simple-row",
                                                    div { class: "hint", "{check.name}: {check.status}" }
                                                    div { class: "hint", "{check.message}" }
                                                    if let Some(fix) = &check.fix_hint {
                                                        div { class: "hint", "Fix: {fix}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                div { class: "simple-actions",
                                    button {
                                        onclick: move |_| on_save_config.call(()),
                                        "Save Settings"
                                    }
                                    button {
                                        onclick: move |_| on_factory_reset_config.call(()),
                                        "Factory Reset Defaults"
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
