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
use tokio::fs;
use tokio::time::{sleep, timeout, Duration};

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

#[derive(Clone)]
struct ChatMessage {
    id: u64,
    role: MessageRole,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageRole {
    User,
    Bot,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UiTab {
    Chat,
    Config,
    Skill,
    Heartbeat,
    Prompt,
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
    fs::read_to_string(trimmed)
        .await
        .map_err(|err| err.to_string())
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

async fn scroll_chat_to_bottom() {
    let _ = eval(
        "const el = document.getElementById('chat-scroll'); if (el) { el.scrollTop = el.scrollHeight; }",
    )
    .await;
}

async fn scroll_chat_after_render() {
    scroll_chat_to_bottom().await;
    sleep(Duration::from_millis(16)).await;
    scroll_chat_to_bottom().await;
}

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
        env::var("BUTTERFLY_BOT_DB").unwrap_or_else(|_| "./data/butterfly-bot.db".to_string());
    let token = env::var("BUTTERFLY_BOT_TOKEN").unwrap_or_default();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let thread = thread::spawn(move || {
        if let Ok(runtime) = tokio::runtime::Runtime::new() {
            runtime.block_on(async move {
                let shutdown = async move {
                    let _ = shutdown_rx.await;
                };
                let _ =
                    crate::daemon::run_with_shutdown(&host, port, &db_path, &token, shutdown)
                        .await;
            });
        }
    });

    *guard = Some(DaemonControl {
        shutdown: shutdown_tx,
        thread,
    });

    Ok(())
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
        env::var("BUTTERFLY_BOT_DB").unwrap_or_else(|_| "./data/butterfly-bot.db".to_string());
    let daemon_url = use_signal(|| {
        let raw =
            env::var("BUTTERFLY_BOT_DAEMON").unwrap_or_else(|_| "http://127.0.0.1:7878".to_string());
        normalize_daemon_url(&raw)
    });
    let token = use_signal(|| env::var("BUTTERFLY_BOT_TOKEN").unwrap_or_default());
    let user_id =
        use_signal(|| env::var("BUTTERFLY_BOT_USER_ID").unwrap_or_else(|_| "cli_user".to_string()));
    let prompt = use_signal(String::new);
    let input = use_signal(String::new);
    let busy = use_signal(|| false);
    let error = use_signal(String::new);
    let messages = use_signal(Vec::<ChatMessage>::new);
    let daemon_running = use_signal(|| false);
    let daemon_status = use_signal(String::new);
    let next_id = use_signal(|| 1u64);
    let active_tab = use_signal(|| UiTab::Chat);
    let reminders_listening = use_signal(|| false);
    let reminders_listener_started = use_signal(|| false);
    let ui_events_listening = use_signal(|| false);
    let ui_events_listener_started = use_signal(|| false);

    let tools_loaded = use_signal(|| false);
    let settings_load_started = use_signal(|| false);
    let boot_ready = use_signal(|| false);
    let boot_status = use_signal(String::new);
    let boot_skill_ready = use_signal(|| false);
    let boot_heartbeat_ready = use_signal(|| false);
    let settings_error = use_signal(String::new);
    let settings_status = use_signal(String::new);
    let doctor_status = use_signal(String::new);
    let doctor_error = use_signal(String::new);
    let doctor_running = use_signal(|| false);
    let doctor_overall = use_signal(String::new);
    let doctor_checks = use_signal(Vec::<DoctorCheckResponse>::new);
    let config_json_text = use_signal(String::new);
    let skill_text = use_signal(String::new);
    let skill_path = use_signal(|| "./skill.md".to_string());
    let skill_status = use_signal(String::new);
    let skill_error = use_signal(String::new);
    let heartbeat_text = use_signal(String::new);
    let heartbeat_path = use_signal(|| "./heartbeat.md".to_string());
    let heartbeat_status = use_signal(String::new);
    let heartbeat_error = use_signal(String::new);
    let prompt_text = use_signal(String::new);
    let prompt_path = use_signal(|| "./prompt.md".to_string());
    let prompt_status = use_signal(String::new);
    let prompt_error = use_signal(String::new);

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
        let prompt = prompt.clone();
        let input = input.clone();
        let busy = busy.clone();
        let error = error.clone();
        let messages = messages.clone();
        let next_id = next_id.clone();
        let boot_ready = boot_ready.clone();

        use_callback(move |_| {
            let daemon_url = daemon_url();
            let token = token();
            let user_id = user_id();
            let prompt = prompt();
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

                if !*boot_ready.read() {
                    error.set("Initializing skill/heartbeat. Please wait...".to_string());
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

                messages.write().push(ChatMessage {
                    id: user_message_id,
                    role: MessageRole::User,
                    text: text.clone(),
                });
                messages.write().push(ChatMessage {
                    id: bot_message_id,
                    role: MessageRole::Bot,
                    text: String::new(),
                });

                input.set(String::new());
                scroll_chat_after_render().await;

                let client = reqwest::Client::new();
                let url = format!("{}/process_text_stream", daemon_url.trim_end_matches('/'));
                let body = ProcessTextRequest {
                    user_id,
                    text,
                    prompt: if prompt.trim().is_empty() {
                        None
                    } else {
                        Some(prompt)
                    },
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
                                                    last.text.push_str(text_chunk);
                                                }
                                            }
                                        }
                                        scroll_chat_to_bottom().await;
                                    }
                                    Err(err) => {
                                        error.set(format!("Stream error: {err}"));
                                        break;
                                    }
                                }
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
                        let _ = error.set(format!(
                            "Request failed: {err}. Daemon unreachable at {daemon_url}. Use Start in Config > Daemon."
                        ));
                    }
                }

                busy.set(false);
            });
        })
    };
    let on_send_key = on_send.clone();

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

        use_callback(move |_| {
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
                        daemon_running.set(true);
                        daemon_status.set("Daemon started.".to_string());
                        boot_ready.set(false);
                        boot_status.set("Starting daemon…".to_string());

                        // Wait for daemon to be ready (retry up to 10 times with 500ms delay)
                        let client = reqwest::Client::new();
                        let mut daemon_ready = false;
                        for i in 0..10 {
                            sleep(Duration::from_millis(500)).await;
                            let health_url = format!("{}/health", daemon_url().trim_end_matches('/'));
                            if let Ok(resp) = client.get(&health_url).send().await {
                                if resp.status().is_success() {
                                    daemon_ready = true;
                                    break;
                                }
                            }
                            boot_status.set(format!("Waiting for daemon... ({}/10)", i + 1));
                        }

                        if !daemon_ready {
                            boot_status.set("Daemon started but not responding. Continuing without preload.".to_string());
                            boot_ready.set(true);
                        } else {
                            let url = format!(
                                "{}/preload_boot",
                                daemon_url().trim_end_matches('/')
                            );
                            let mut request = client.post(&url).json(&PreloadBootRequest {
                                user_id: user_id(),
                            });
                            let token_value = token();
                            if !token_value.trim().is_empty() {
                                request =
                                    request.header("authorization", format!("Bearer {token_value}"));
                            }
                            match request.send().await {
                                Ok(resp) if resp.status().is_success() => {
                                    boot_status.set("Boot preload started…".to_string());
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
                                    doctor_status.set(format!(
                                        "Diagnostics complete ({overall})."
                                    ));
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

        use_callback(move |_| {
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
                let result = stop_local_daemon();
                match result {
                    Ok(()) => {
                        daemon_running.set(false);
                        reminders_listening.set(false);
                        ui_events_listening.set(false);
                        boot_ready.set(false);
                        boot_status.set("Daemon stopped. Start it to preload skill + heartbeat.".to_string());
                        daemon_status.set("Daemon stopped.".to_string());
                        doctor_status.set(String::new());
                        doctor_error.set(String::new());
                        doctor_overall.set(String::new());
                        doctor_checks.set(Vec::new());
                    }
                    Err(err) => {
                        daemon_status.set(err);
                    }
                }
            });
        })
    };

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
                    break;
                }
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
                                    messages.write().push(ChatMessage {
                                        id,
                                        role: MessageRole::Bot,
                                        text: format!("⏰ {title}"),
                                    });
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
        let ui_events_listener_started = ui_events_listener_started.clone();
        let ui_events_listening = ui_events_listening.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let user_id = user_id.clone();
        let messages = messages.clone();
        let next_id = next_id.clone();
        let daemon_running = daemon_running.clone();
        let mut boot_ready = boot_ready.clone();
        let mut boot_status = boot_status.clone();
        let mut boot_skill_ready = boot_skill_ready.clone();
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
                let mut messages = messages;
                let mut next_id = next_id;

                ui_events_listening.set(true);
                let client = reqwest::Client::new();
                loop {
                if !*daemon_running.read() {
                    ui_events_listening.set(false);
                    break;
                }
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

                                    // Always update boot readiness for boot/skill/heartbeat events
                                    if (event_type == "boot" || tool == "skill") && status == "ok" {
                                        boot_skill_ready.set(true);
                                    }
                                    if (event_type == "boot" || tool == "heartbeat") && status == "ok" {
                                        boot_heartbeat_ready.set(true);
                                    }
                                    if *boot_skill_ready.read() && *boot_heartbeat_ready.read() {
                                        boot_ready.set(true);
                                        let _ = boot_status.set("Skill + heartbeat ready".to_string());
                                    }

                                    let show_success =
                                        std::env::var("BUTTERFLY_BOT_SHOW_TOOL_SUCCESS").is_ok();
                                    if event_type != "boot"
                                        && event_type != "autonomy"
                                        && event_type != "tool"
                                        && !show_success
                                        && (status == "success" || status == "ok")
                                    {
                                        if let Some(payload) = value.get("payload") {
                                            if payload.get("error").is_none() {
                                                continue;
                                            }
                                        } else {
                                            continue;
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
                                    messages.write().push(ChatMessage {
                                        id,
                                        role: MessageRole::Bot,
                                        text,
                                    });
                                    scroll_chat_to_bottom().await;
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
        let skill_text = skill_text.clone();
        let skill_path = skill_path.clone();
        let skill_error = skill_error.clone();
        let heartbeat_text = heartbeat_text.clone();
        let heartbeat_path = heartbeat_path.clone();
        let heartbeat_error = heartbeat_error.clone();
        let prompt_text = prompt_text.clone();
        let prompt_path = prompt_path.clone();
        let prompt_error = prompt_error.clone();
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
                let mut skill_text = skill_text;
                let mut skill_path = skill_path;
                let mut skill_error = skill_error;
                let mut heartbeat_text = heartbeat_text;
                let mut heartbeat_path = heartbeat_path;
                let heartbeat_error = heartbeat_error;
                let mut boot_status = boot_status;
                let mut boot_ready = boot_ready;
                let prompt_path = prompt_path;
                let prompt_text = prompt_text;
                let prompt_error = prompt_error;

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
                            if let Some(value) = perms.get("default_deny").and_then(|v| v.as_bool())
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
                if let Some(search_cfg) = tools_value.get("search_internet") {
                    if let Some(provider) = search_cfg.get("provider").and_then(|v| v.as_str()) {
                        search_provider.set(provider.to_string());
                    }
                    if let Some(model) = search_cfg.get("model").and_then(|v| v.as_str()) {
                        search_model.set(model.to_string());
                    }
                    if let Some(citations) = search_cfg.get("citations").and_then(|v| v.as_bool()) {
                        search_citations.set(citations);
                    }
                    if let Some(web) = search_cfg.get("grok_web_search").and_then(|v| v.as_bool()) {
                        search_grok_web.set(web);
                    }
                    if let Some(x_search) =
                        search_cfg.get("grok_x_search").and_then(|v| v.as_bool())
                    {
                        search_grok_x.set(x_search);
                    }
                    if let Some(timeout) = search_cfg.get("grok_timeout").and_then(|v| v.as_u64()) {
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
                    if let Some(path) = reminders_cfg.get("sqlite_path").and_then(|v| v.as_str()) {
                        reminders_sqlite_path.set(path.to_string());
                    }
                }
            }

            let skill_source = config
                .skill_file
                .clone()
                .unwrap_or_else(|| "./skill.md".to_string());
            skill_path.set(skill_source.clone());
            match load_markdown_source(&skill_source).await {
                Ok(text) => skill_text.set(text),
                Err(err) => skill_error.set(format!("Skill file error: {err}")),
            }

            let heartbeat_source = config
                .heartbeat_file
                .clone()
                .unwrap_or_else(|| "./heartbeat.md".to_string());
            let mut heartbeat_error = heartbeat_error;
            heartbeat_path.set(heartbeat_source.clone());
            match load_markdown_source(&heartbeat_source).await {
                Ok(text) => heartbeat_text.set(text),
                Err(err) => heartbeat_error.set(format!("Heartbeat error: {err}")),
            }

            let mut prompt_path = prompt_path;
            let mut prompt_text = prompt_text;
            let mut prompt_error = prompt_error;
            let prompt_source = config
                .prompt_file
                .clone()
                .unwrap_or_else(|| "./prompt.md".to_string());
            prompt_path.set(prompt_source.clone());
            match load_markdown_source(&prompt_source).await {
                Ok(text) => prompt_text.set(text),
                Err(err) => prompt_error.set(format!("Prompt error: {err}")),
            }

            search_default_deny.set(default_deny);
            if !allowlist.is_empty() {
                search_network_allow.set(allowlist.join(", "));
            }

            let provider_name = search_provider();
            let secret_name = match provider_name.as_str() {
                "perplexity" => "search_internet_perplexity_api_key",
                "grok" => "search_internet_grok_api_key",
                _ => "search_internet_openai_api_key",
            };
            match crate::vault::get_secret(secret_name) {
                Ok(Some(secret)) if !secret.trim().is_empty() => {
                    search_api_key_status.set("Stored in vault".to_string());
                }
                Ok(_) => {
                    search_api_key_status.set("Not set".to_string());
                }
                Err(err) => {
                    search_api_key_status.set(format!("Vault error: {err}"));
                }
            }

            if !*daemon_running.read() {
                boot_status.set("Daemon is stopped. Start it to preload skill + heartbeat.".to_string());
            } else {
                // Preload skill into memory and heartbeat into agent.
                boot_status.set("Initializing skill + heartbeat...".to_string());
                let client = reqwest::Client::new();
                let url = format!("{}/preload_boot", daemon_url().trim_end_matches('/'));
                let mut request = client.post(&url).json(&PreloadBootRequest {
                    user_id: user_id(),
                });
                let token_value = token();
                if !token_value.trim().is_empty() {
                    request = request.header("authorization", format!("Bearer {token_value}"));
                }
                match request.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        boot_status.set("Boot preload started...".to_string());
                        let boot_ready = boot_ready.clone();
                        let boot_status = boot_status.clone();
                        spawn(async move {
                            let mut boot_ready = boot_ready;
                            let mut boot_status = boot_status;
                            sleep(Duration::from_secs(15)).await;
                            if !*boot_ready.read() {
                                boot_status.set("Boot preload timed out; continuing".to_string());
                                boot_ready.set(true);
                            }
                        });
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        boot_status.set(format!("Boot preload failed: HTTP {status}. Continuing without preload."));
                        boot_ready.set(true);
                    }
                    Err(err) => {
                        boot_status.set(format!("Boot preload error: {err}. Continuing without preload."));
                        boot_ready.set(true);
                    }
                }
            }

            tools_loaded.set(true);
            });
        });
    }

    let on_format_config = {
        let settings_error = settings_error.clone();
        let settings_status = settings_status.clone();
        let config_json_text = config_json_text.clone();

        use_callback(move |_| {
            let settings_error = settings_error.clone();
            let settings_status = settings_status.clone();
            let config_json_text = config_json_text.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut settings_status = settings_status;
                let mut config_json_text = config_json_text;

                settings_error.set(String::new());
                settings_status.set(String::new());

                let raw = config_json_text();
                match serde_json::from_str::<Value>(&raw) {
                    Ok(value) => {
                        let pretty = serde_json::to_string_pretty(&value).unwrap_or(raw);
                        config_json_text.set(pretty);
                        settings_status.set("Formatted JSON.".to_string());
                    }
                    Err(err) => {
                        settings_error.set(format!("Invalid JSON: {err}"));
                    }
                }
            });
        })
    };

    let on_validate_config = {
        let settings_error = settings_error.clone();
        let settings_status = settings_status.clone();
        let config_json_text = config_json_text.clone();

        use_callback(move |_| {
            let settings_error = settings_error.clone();
            let settings_status = settings_status.clone();
            let config_json_text = config_json_text.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut settings_status = settings_status;
                let config_json_text = config_json_text;

                settings_error.set(String::new());
                settings_status.set(String::new());

                let raw = config_json_text();
                let value: Value = match serde_json::from_str(&raw) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Invalid JSON: {err}"));
                        return;
                    }
                };
                match serde_json::from_value::<crate::config::Config>(value) {
                    Ok(_) => settings_status.set("Config is valid.".to_string()),
                    Err(err) => settings_error.set(format!("Invalid config: {err}")),
                }
            });
        })
    };

    let on_save_config = {
        let settings_error = settings_error.clone();
        let settings_status = settings_status.clone();
        let config_json_text = config_json_text.clone();
        let db_path = db_path.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let doctor_status = doctor_status.clone();
        let doctor_error = doctor_error.clone();
        let doctor_running = doctor_running.clone();
        let doctor_overall = doctor_overall.clone();
        let doctor_checks = doctor_checks.clone();

        use_callback(move |_| {
            let settings_error = settings_error.clone();
            let settings_status = settings_status.clone();
            let config_json_text = config_json_text.clone();
            let db_path = db_path.clone();
            let daemon_url = daemon_url.clone();
            let token = token.clone();
            let doctor_status = doctor_status.clone();
            let doctor_error = doctor_error.clone();
            let doctor_running = doctor_running.clone();
            let doctor_overall = doctor_overall.clone();
            let doctor_checks = doctor_checks.clone();

            spawn(async move {
                let mut settings_error = settings_error;
                let mut settings_status = settings_status;
                let mut config_json_text = config_json_text;
                let mut doctor_status = doctor_status;
                let mut doctor_error = doctor_error;
                let mut doctor_running = doctor_running;
                let mut doctor_overall = doctor_overall;
                let mut doctor_checks = doctor_checks;

                settings_error.set(String::new());
                settings_status.set(String::new());

                let raw = config_json_text();
                let value: Value = match serde_json::from_str(&raw) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Invalid JSON: {err}"));
                        return;
                    }
                };
                let config: crate::config::Config = match serde_json::from_value(value.clone()) {
                    Ok(value) => value,
                    Err(err) => {
                        settings_error.set(format!("Invalid config: {err}"));
                        return;
                    }
                };

                let pretty = serde_json::to_string_pretty(&value).unwrap_or(raw.clone());
                if let Err(err) = crate::vault::set_secret("app_config_json", &pretty) {
                    settings_error.set(format!("Failed to store config in keyring: {err}"));
                    return;
                }

                let result = tokio::task::spawn_blocking(move || {
                    crate::config_store::save_config(&db_path, &config)
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
                                settings_status.set("Config saved and reloaded.".to_string());
                                doctor_running.set(true);
                                doctor_error.set(String::new());
                                doctor_status.set("Running diagnostics…".to_string());
                                match run_doctor_request(daemon_url(), token()).await {
                                    Ok(report) => {
                                        let overall = report.overall.clone();
                                        doctor_overall.set(overall.clone());
                                        doctor_checks.set(report.checks);
                                        doctor_status.set(format!(
                                            "Diagnostics complete ({overall})."
                                        ));
                                    }
                                    Err(err) => {
                                        doctor_error.set(format!("Diagnostics failed: {err}"));
                                        doctor_status.set(String::new());
                                    }
                                }
                                doctor_running.set(false);
                            }
                            Ok(response) => {
                                let status = response.status();
                                let text = response
                                    .text()
                                    .await
                                    .unwrap_or_else(|_| "Unable to read error body".to_string());
                                settings_status.set(format!(
                                    "Config saved, but reload failed ({status}). Restart required. {text}"
                                ));
                            }
                            Err(err) => {
                                settings_status.set(format!(
                                    "Config saved, but reload failed: {err}. Restart required."
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

    let on_run_doctor = {
        let daemon_running = daemon_running.clone();
        let daemon_url = daemon_url.clone();
        let token = token.clone();
        let doctor_status = doctor_status.clone();
        let doctor_error = doctor_error.clone();
        let doctor_running = doctor_running.clone();
        let doctor_overall = doctor_overall.clone();
        let doctor_checks = doctor_checks.clone();

        use_callback(move |_| {
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

                if !*daemon_running.read() {
                    doctor_error.set("Daemon is not running. Start daemon first.".to_string());
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
                    }
                }

                doctor_running.set(false);
            });
        })
    };

    let on_validate_skill = {
        let skill_text = skill_text.clone();
        let skill_path = skill_path.clone();
        let skill_status = skill_status.clone();
        let skill_error = skill_error.clone();

        use_callback(move |_| {
            let skill_text = skill_text.clone();
            let skill_path = skill_path.clone();
            let skill_status = skill_status.clone();
            let skill_error = skill_error.clone();

            spawn(async move {
                let mut skill_status = skill_status;
                let mut skill_error = skill_error;

                skill_status.set(String::new());
                skill_error.set(String::new());

                let source = skill_path();
                if source.trim().is_empty() {
                    skill_error.set("Skill file path is empty.".to_string());
                    return;
                }

                if is_url_source(&source) {
                    match load_markdown_source(&source).await {
                        Ok(text) if !text.trim().is_empty() => {
                            skill_status.set("Skill URL is reachable.".to_string())
                        }
                        Ok(_) => skill_error.set("Skill URL returned empty content.".to_string()),
                        Err(err) => skill_error.set(format!("Skill URL error: {err}")),
                    }
                    return;
                }

                let content = skill_text();
                if content.trim().is_empty() {
                    skill_error.set("Skill markdown is empty.".to_string());
                    return;
                }
                skill_status.set("Skill markdown looks valid.".to_string());
            });
        })
    };

    let on_save_skill = {
        let skill_text = skill_text.clone();
        let skill_path = skill_path.clone();
        let skill_status = skill_status.clone();
        let skill_error = skill_error.clone();

        use_callback(move |_| {
            let skill_text = skill_text.clone();
            let skill_path = skill_path.clone();
            let skill_status = skill_status.clone();
            let skill_error = skill_error.clone();

            spawn(async move {
                let mut skill_status = skill_status;
                let mut skill_error = skill_error;

                skill_status.set(String::new());
                skill_error.set(String::new());

                let source = skill_path();
                if source.trim().is_empty() {
                    skill_error.set("Skill file path is empty.".to_string());
                    return;
                }
                if is_url_source(&source) {
                    skill_error.set("Skill source is a URL and cannot be saved here.".to_string());
                    return;
                }

                let content = skill_text();
                if content.trim().is_empty() {
                    skill_error.set("Skill markdown is empty.".to_string());
                    return;
                }

                if let Err(err) = fs::write(&source, content).await {
                    skill_error.set(format!("Failed to save skill file: {err}"));
                    return;
                }
                skill_status.set("Skill file saved.".to_string());
            });
        })
    };

    let on_validate_heartbeat = {
        let heartbeat_text = heartbeat_text.clone();
        let heartbeat_path = heartbeat_path.clone();
        let heartbeat_status = heartbeat_status.clone();
        let heartbeat_error = heartbeat_error.clone();

        use_callback(move |_| {
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

        use_callback(move |_| {
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
                    heartbeat_error
                        .set("Heartbeat source is a URL and cannot be saved here.".to_string());
                    return;
                }

                let content = heartbeat_text();
                if content.trim().is_empty() {
                    heartbeat_error.set("Heartbeat markdown is empty.".to_string());
                    return;
                }

                if let Err(err) = fs::write(&source, content).await {
                    heartbeat_error.set(format!("Failed to save heartbeat file: {err}"));
                    return;
                }
                heartbeat_status.set("Heartbeat file saved.".to_string());
            });
        })
    };

    let on_validate_prompt = {
        let prompt_text = prompt_text.clone();
        let prompt_path = prompt_path.clone();
        let prompt_status = prompt_status.clone();
        let prompt_error = prompt_error.clone();

        use_callback(move |_| {
            let prompt_text = prompt_text.clone();
            let prompt_path = prompt_path.clone();
            let prompt_status = prompt_status.clone();
            let prompt_error = prompt_error.clone();

            spawn(async move {
                let mut prompt_status = prompt_status;
                let mut prompt_error = prompt_error;

                prompt_status.set(String::new());
                prompt_error.set(String::new());

                let source = prompt_path();
                if source.trim().is_empty() {
                    prompt_error.set("Prompt path or URL is empty.".to_string());
                    return;
                }

                if is_url_source(&source) {
                    match load_markdown_source(&source).await {
                        Ok(text) if !text.trim().is_empty() => {
                            prompt_status.set("Prompt URL is reachable.".to_string())
                        }
                        Ok(_) => prompt_error.set("Prompt URL returned empty content.".to_string()),
                        Err(err) => prompt_error.set(format!("Prompt URL error: {err}")),
                    }
                    return;
                }

                let content = prompt_text();
                if content.trim().is_empty() {
                    prompt_error.set("Prompt markdown is empty.".to_string());
                    return;
                }
                prompt_status.set("Prompt markdown looks valid.".to_string());
            });
        })
    };

    let on_save_prompt = {
        let prompt_text = prompt_text.clone();
        let prompt_path = prompt_path.clone();
        let prompt_status = prompt_status.clone();
        let prompt_error = prompt_error.clone();

        use_callback(move |_| {
            let prompt_text = prompt_text.clone();
            let prompt_path = prompt_path.clone();
            let prompt_status = prompt_status.clone();
            let prompt_error = prompt_error.clone();

            spawn(async move {
                let mut prompt_status = prompt_status;
                let mut prompt_error = prompt_error;

                prompt_status.set(String::new());
                prompt_error.set(String::new());

                let source = prompt_path();
                if source.trim().is_empty() {
                    prompt_error.set("Prompt path or URL is empty.".to_string());
                    return;
                }
                if is_url_source(&source) {
                    prompt_error.set("Prompt source is a URL and cannot be saved here.".to_string());
                    return;
                }

                let content = prompt_text();
                if content.trim().is_empty() {
                    prompt_error.set("Prompt markdown is empty.".to_string());
                    return;
                }

                if let Err(err) = fs::write(&source, content).await {
                    prompt_error.set(format!("Failed to save prompt file: {err}"));
                    return;
                }
                prompt_status.set("Prompt file saved.".to_string());
            });
        })
    };

    let active_tab_chat = active_tab.clone();
    let active_tab_config = active_tab.clone();
    let active_tab_skill = active_tab.clone();
    let active_tab_heartbeat = active_tab.clone();
    let active_tab_prompt = active_tab.clone();
    let prompt_input = prompt.clone();
    let message_input = input.clone();

    rsx! {
        style { r#"
            body {{
                font-family: system-ui, -apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", sans-serif;
                background: radial-gradient(1200px 800px at 20% -10%, rgba(120,119,198,0.35), transparent 60%),
                            radial-gradient(1000px 700px at 110% 10%, rgba(56,189,248,0.25), transparent 60%),
                            #0b1020;
                color: #e5e7eb;
            }}
            .container {{ max-width: 980px; margin: 0 auto; padding: 0; height: 100vh; display: flex; flex-direction: column; }}
            .header {{
                padding: 16px 20px;
                background: rgba(17,24,39,0.55);
                color: #e5e7eb;
                display: flex; align-items: center; justify-content: space-between;
                border-bottom: 1px solid rgba(255,255,255,0.08);
                backdrop-filter: blur(18px) saturate(180%);
                box-shadow: 0 8px 30px rgba(0,0,0,0.25);
            }}
            .nav {{ display: flex; gap: 8px; }}
            .nav button {{ background: rgba(255,255,255,0.08); }}
            .nav button.active {{ background: rgba(99,102,241,0.6); }}
            .title {{ font-size: 18px; font-weight: 700; letter-spacing: 0.2px; }}
            .chat {{ flex: 1; min-height: 0; overflow-y: auto; padding: 20px; background: transparent; }}
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
            .composer {{
                padding: 16px 20px;
                background: rgba(17,24,39,0.55);
                border-top: 1px solid rgba(255,255,255,0.08);
                display: flex; flex-direction: column; gap: 12px;
                position: sticky; bottom: 0;
                backdrop-filter: blur(18px) saturate(180%);
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
            input, textarea {{
                width: 100%; padding: 10px 12px; border-radius: 12px;
                border: 1px solid rgba(255,255,255,0.12);
                background: rgba(15,23,42,0.55);
                color: #e5e7eb;
                backdrop-filter: blur(12px) saturate(180%);
                box-shadow: inset 0 1px 0 rgba(255,255,255,0.06);
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
            .status {{ color: #34d399; font-weight: 600; }}
        "# }
        div { class: "container",
            div { class: "header",
                div { class: "title", "ButterFly Bot" }
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
                        class: if *active_tab.read() == UiTab::Config { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_config = active_tab_config.clone();
                            active_tab_config.set(UiTab::Config);
                        },
                        "Config"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Skill { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_skill = active_tab_skill.clone();
                            active_tab_skill.set(UiTab::Skill);
                        },
                        "Skill"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Heartbeat { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_heartbeat = active_tab_heartbeat.clone();
                            active_tab_heartbeat.set(UiTab::Heartbeat);
                        },
                        "Heartbeat"
                    }
                    button {
                        class: if *active_tab.read() == UiTab::Prompt { "active" } else { "" },
                        onclick: move |_| {
                            let mut active_tab_prompt = active_tab_prompt.clone();
                            active_tab_prompt.set(UiTab::Prompt);
                        },
                        "Prompt"
                    }
                }
            }
            if !error.read().is_empty() {
                div { class: "error", "{error}" }
            }
            if *active_tab.read() == UiTab::Chat {
                if !*boot_ready.read() && !boot_status.read().is_empty() {
                    div { class: "hint", "{boot_status}" }
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
                            dangerous_inner_html: markdown_to_html(&message.text),
                        }
                    }
                    if *busy.read() {
                        div { class: "hint", "Bot is typing…" }
                    }
                }
                div { class: "composer",
                    div {
                        label { "System Prompt (optional)" }
                        input {
                            value: "{prompt}",
                            oninput: move |evt| {
                                let mut prompt_input = prompt_input.clone();
                                prompt_input.set(evt.value());
                            },
                        }
                    }
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
                                disabled: *busy.read() || !*boot_ready.read(),
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
                            label { "Daemon" }
                            p { class: "hint", "Start/stop the local daemon for this UI session." }
                            div { class: "config-actions",
                                button {
                                    onclick: move |_| on_daemon_start.call(()),
                                    disabled: *daemon_running.read(),
                                    "Start"
                                }
                                button {
                                    onclick: move |_| on_daemon_stop.call(()),
                                    disabled: !*daemon_running.read(),
                                    "Stop"
                                }
                            }
                            if !daemon_status.read().is_empty() {
                                p { class: "hint", "{daemon_status}" }
                            }
                        }
                        div { class: "settings-card",
                            label { "Config (JSON)" }
                            div { class: "config-head",
                                label { "Editor" }
                                label { "Preview" }
                            }
                            div { class: "config-grid",
                                div { class: "config-panel",
                                    textarea {
                                        id: "config-json",
                                        value: "{config_json_text}",
                                        rows: "18",
                                        class: "config-editor",
                                        oninput: move |evt| {
                                            let mut config_json_text = config_json_text.clone();
                                            config_json_text.set(evt.value());
                                        },
                                    }
                                }
                                div { class: "config-panel",
                                    pre {
                                        class: "config-preview",
                                        dangerous_inner_html: "{highlight_json_html(&config_json_text.read())}",
                                    }
                                }
                            }
                            div { class: "config-actions",
                                button { onclick: move |_| on_format_config.call(()), "Format JSON" }
                                button { onclick: move |_| on_validate_config.call(()), "Validate" }
                                button { onclick: move |_| on_save_config.call(()), "Save Config" }
                            }
                            p { class: "hint", "Saved to the OS keyring. Changes reload automatically." }
                        }
                        div { class: "settings-card",
                            label { "Diagnostics" }
                            p { class: "hint", "Runs config, vault, DB, provider, and daemon auth checks." }
                            div { class: "config-actions",
                                button {
                                    onclick: move |_| on_run_doctor.call(()),
                                    disabled: *doctor_running.read() || !*daemon_running.read(),
                                    if *doctor_running.read() { "Running…" } else { "Run Diagnostics" }
                                }
                            }
                            if !doctor_overall.read().is_empty() {
                                p { class: "hint", "Overall: {doctor_overall}" }
                            }
                            if !doctor_checks.read().is_empty() {
                                div {
                                    class: "tool-list",
                                    for check in doctor_checks.read().iter() {
                                        div {
                                            class: "settings-card",
                                            label { "{check.name}" }
                                            p { class: "hint", "Status: {check.status}" }
                                            p { class: "hint", "{check.message}" }
                                            if let Some(hint) = &check.fix_hint {
                                                p { class: "hint", "Fix: {hint}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !settings_error.read().is_empty() {
                            div { class: "error", "{settings_error}" }
                        }
                        if !settings_status.read().is_empty() {
                            div { class: "status", "{settings_status}" }
                        }
                        if !doctor_error.read().is_empty() {
                            div { class: "error", "{doctor_error}" }
                        }
                        if !doctor_status.read().is_empty() {
                            div { class: "status", "{doctor_status}" }
                        }
                    }
                }
            }
            if *active_tab.read() == UiTab::Skill {
                div { class: "settings",
                    div { class: "settings-card",
                        label { "Skill (Markdown)" }
                        p { class: "hint", "Source: {skill_path}" }
                        div { class: "config-head",
                            label { "Editor" }
                            label { "Preview" }
                        }
                        div { class: "config-grid",
                            div { class: "config-panel",
                                textarea {
                                    id: "skill-md",
                                    value: "{skill_text}",
                                    rows: "18",
                                    class: "config-editor",
                                    oninput: move |evt| {
                                        let mut skill_text = skill_text.clone();
                                        skill_text.set(evt.value());
                                    },
                                }
                            }
                            div { class: "config-panel",
                                div {
                                    class: "config-preview",
                                    dangerous_inner_html: "{markdown_to_html(&skill_text.read())}",
                                }
                            }
                        }
                        div { class: "config-actions",
                            button { onclick: move |_| on_validate_skill.call(()), "Validate" }
                            button {
                                disabled: is_url_source(&skill_path.read()),
                                onclick: move |_| on_save_skill.call(()),
                                "Save Skill"
                            }
                        }
                        if is_url_source(&skill_path.read()) {
                            p { class: "hint", "Remote URL sources are read-only." }
                        }
                    }
                    if !skill_error.read().is_empty() {
                        div { class: "error", "{skill_error}" }
                    }
                    if !skill_status.read().is_empty() {
                        div { class: "status", "{skill_status}" }
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
                    if !heartbeat_status.read().is_empty() {
                        div { class: "status", "{heartbeat_status}" }
                    }
                }
            }
            if *active_tab.read() == UiTab::Prompt {
                div { class: "settings",
                    div { class: "settings-card",
                        label { "Prompt (Markdown)" }
                        p { class: "hint", "Source: {prompt_path}" }
                        div { class: "config-head",
                            label { "Editor" }
                            label { "Preview" }
                        }
                        div { class: "config-grid",
                            div { class: "config-panel",
                                textarea {
                                    id: "prompt-md",
                                    value: "{prompt_text}",
                                    rows: "18",
                                    class: "config-editor",
                                    oninput: move |evt| {
                                        let mut prompt_text = prompt_text.clone();
                                        prompt_text.set(evt.value());
                                    },
                                }
                            }
                            div { class: "config-panel",
                                div {
                                    class: "config-preview",
                                    dangerous_inner_html: "{markdown_to_html(&prompt_text.read())}",
                                }
                            }
                        }
                        div { class: "config-actions",
                            button { onclick: move |_| on_validate_prompt.call(()), "Validate" }
                            button {
                                disabled: is_url_source(&prompt_path.read()),
                                onclick: move |_| on_save_prompt.call(()),
                                "Save Prompt"
                            }
                        }
                        if is_url_source(&prompt_path.read()) {
                            p { class: "hint", "Remote URL sources are read-only." }
                        }
                    }
                    if !prompt_error.read().is_empty() {
                        div { class: "error", "{prompt_error}" }
                    }
                    if !prompt_status.read().is_empty() {
                        div { class: "status", "{prompt_status}" }
                    }
                }
            }
        }
    }
}
