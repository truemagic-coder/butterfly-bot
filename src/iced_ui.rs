use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input};
use iced::{application, time, Element, Length, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct IcedUiLaunchConfig {
    pub daemon_url: String,
    pub user_id: String,
    pub db_path: String,
}

#[derive(Clone, Serialize)]
struct ProcessTextRequest {
    user_id: String,
    text: String,
    prompt: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DoctorCheckResponse {
    name: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DoctorResponse {
    overall: String,
    checks: Vec<DoctorCheckResponse>,
}

#[derive(Clone, Debug, Deserialize)]
struct SecurityAuditFindingResponse {
    id: String,
    severity: String,
    status: String,
    message: String,
    fix_hint: Option<String>,
    auto_fixable: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct SecurityAuditResponse {
    overall: String,
    findings: Vec<SecurityAuditFindingResponse>,
}

#[derive(Clone)]
struct ChatMessage {
    role: MessageRole,
    text: String,
    timestamp: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageRole {
    User,
    Bot,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiTab {
    Chat,
    Activity,
    Settings,
    Diagnostics,
}

impl UiTab {
    fn all() -> [UiTab; 4] {
        [
            UiTab::Chat,
            UiTab::Activity,
            UiTab::Settings,
            UiTab::Diagnostics,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            UiTab::Chat => "Chat",
            UiTab::Activity => "Activity",
            UiTab::Settings => "Settings",
            UiTab::Diagnostics => "Diagnostics",
        }
    }
}

#[derive(Clone, Debug)]
struct SettingsForm {
    github_pat: String,
    zapier_token: String,
    openai_api_key: String,
    search_api_key: String,
    openai_base_url: String,
    openai_model: String,
    prompt_text: String,
    heartbeat_text: String,
    memory_enabled: bool,
}

impl Default for SettingsForm {
    fn default() -> Self {
        Self {
            github_pat: String::new(),
            zapier_token: String::new(),
            openai_api_key: String::new(),
            search_api_key: String::new(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            openai_model: "gpt-4.1-mini".to_string(),
            prompt_text: String::new(),
            heartbeat_text: String::new(),
            memory_enabled: true,
        }
    }
}

#[derive(Clone, Debug)]
struct LoadedSettings {
    form: SettingsForm,
    pretty_json: String,
}

struct ButterflyIcedApp {
    daemon_url: String,
    user_id: String,
    db_path: String,
    token: String,
    active_tab: UiTab,
    composer: String,
    busy: bool,
    error: String,
    daemon_running: bool,
    daemon_starting: bool,
    daemon_status: String,
    next_id: u64,
    history_loaded: bool,
    chat_messages: Vec<ChatMessage>,
    activity_messages: Vec<ChatMessage>,
    settings: SettingsForm,
    settings_json: String,
    settings_status: String,
    settings_error: String,
    doctor_status: String,
    doctor_error: String,
    doctor_overall: String,
    doctor_checks: Vec<DoctorCheckResponse>,
    security_status: String,
    security_error: String,
    security_overall: String,
    security_findings: Vec<SecurityAuditFindingResponse>,
    manage_local_daemon: bool,
}

#[derive(Clone, Debug)]
enum Message {
    Tick,
    TabSelected(UiTab),
    ComposerChanged(String),
    SendPressed,
    ResponseReady(Result<String, String>),
    HealthChecked(bool),
    StartDaemonPressed,
    StopDaemonPressed,
    DaemonStartFinished(Result<String, String>),
    DaemonStopFinished(Result<String, String>),
    LoadHistoryPressed,
    HistoryLoaded(Result<Vec<String>, String>),
    ClearHistoryPressed,
    HistoryCleared(Result<(), String>),
    LoadSettingsPressed,
    SettingsLoaded(Result<LoadedSettings, String>),
    SaveSettingsPressed,
    SettingsSaved(Result<String, String>),
    GithubPatChanged(String),
    ZapierTokenChanged(String),
    OpenAiKeyChanged(String),
    SearchKeyChanged(String),
    OpenAiBaseUrlChanged(String),
    OpenAiModelChanged(String),
    PromptTextChanged(String),
    HeartbeatTextChanged(String),
    MemoryEnabledChanged(bool),
    RunDoctorPressed,
    DoctorFinished(Result<DoctorResponse, String>),
    RunSecurityPressed,
    SecurityFinished(Result<SecurityAuditResponse, String>),
}

pub fn launch_ui(config: IcedUiLaunchConfig) -> iced::Result {
    let boot_config = config.clone();
    application(
        move || {
            let state = ButterflyIcedApp::new(boot_config.clone());
            let boot = Task::batch(vec![
                Task::perform(
                    check_daemon_health(state.daemon_url.clone()),
                    Message::HealthChecked,
                ),
                Task::perform(load_settings(state.db_path.clone()), Message::SettingsLoaded),
            ]);
            (state, boot)
        },
        update,
        view,
    )
    .title(app_title)
    .theme(app_theme)
    .subscription(subscription)
    .run()
}

fn app_title(_state: &ButterflyIcedApp) -> String {
    "Butterfly Bot".to_string()
}

fn app_theme(_state: &ButterflyIcedApp) -> Theme {
    Theme::Dark
}

fn subscription(_state: &ButterflyIcedApp) -> Subscription<Message> {
    time::every(Duration::from_secs(2)).map(|_| Message::Tick)
}

impl ButterflyIcedApp {
    fn new(flags: IcedUiLaunchConfig) -> Self {
        let manage_local_daemon = env_flag_enabled("BUTTERFLY_UI_MANAGE_DAEMON", false);
        Self {
            daemon_url: normalize_daemon_url(&flags.daemon_url),
            user_id: flags.user_id,
            db_path: flags.db_path,
            token: std::env::var("BUTTERFLY_BOT_TOKEN").unwrap_or_default(),
            active_tab: UiTab::Chat,
            composer: String::new(),
            busy: false,
            error: String::new(),
            daemon_running: false,
            daemon_starting: false,
            daemon_status: if manage_local_daemon {
                "Local daemon control enabled".to_string()
            } else {
                "External daemon mode".to_string()
            },
            next_id: 1,
            history_loaded: false,
            chat_messages: vec![],
            activity_messages: vec![],
            settings: SettingsForm::default(),
            settings_json: String::new(),
            settings_status: String::new(),
            settings_error: String::new(),
            doctor_status: String::new(),
            doctor_error: String::new(),
            doctor_overall: String::new(),
            doctor_checks: vec![],
            security_status: String::new(),
            security_error: String::new(),
            security_overall: String::new(),
            security_findings: vec![],
            manage_local_daemon,
        }
    }

    fn push_chat(&mut self, role: MessageRole, text: String) {
        self.chat_messages.push(ChatMessage {
            role,
            text,
            timestamp: now_unix_ts(),
        });
        if self.chat_messages.len() > 300 {
            let drop_count = self.chat_messages.len() - 300;
            self.chat_messages.drain(0..drop_count);
        }
    }

    fn push_activity(&mut self, text: String) {
        self.activity_messages.push(ChatMessage {
            role: MessageRole::System,
            text,
            timestamp: now_unix_ts(),
        });
        if self.activity_messages.len() > 300 {
            let drop_count = self.activity_messages.len() - 300;
            self.activity_messages.drain(0..drop_count);
        }
    }
}

fn update(state: &mut ButterflyIcedApp, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            if !state.daemon_starting {
                return Task::perform(
                    check_daemon_health(state.daemon_url.clone()),
                    Message::HealthChecked,
                );
            }
            Task::none()
        }
        Message::TabSelected(tab) => {
            state.active_tab = tab;
            Task::none()
        }
        Message::ComposerChanged(value) => {
            state.composer = value;
            Task::none()
        }
        Message::SendPressed => {
            if state.busy || state.composer.trim().is_empty() {
                return Task::none();
            }

            if !state.daemon_running {
                state.error = "Daemon is not healthy. Start/check daemon first.".to_string();
                return Task::none();
            }

            let prompt = state.composer.trim().to_string();
            state.push_chat(MessageRole::User, prompt.clone());
            state.composer.clear();
            state.busy = true;
            state.error.clear();

            let daemon_url = state.daemon_url.clone();
            let user_id = state.user_id.clone();
            let token = state.token.clone();

            Task::perform(send_prompt(daemon_url, user_id, token, prompt), Message::ResponseReady)
        }
        Message::ResponseReady(result) => {
            state.busy = false;
            match result {
                Ok(reply) => state.push_chat(MessageRole::Bot, reply),
                Err(err) => {
                    state.error = err.clone();
                    state.push_activity(format!("chat error: {err}"));
                }
            }
            Task::none()
        }
        Message::HealthChecked(ok) => {
            state.daemon_running = ok;
            if ok {
                if state.daemon_status.is_empty() || state.daemon_status.contains("not reachable") {
                    state.daemon_status = "Daemon healthy".to_string();
                }
                if !state.history_loaded {
                    state.history_loaded = true;
                    return Task::perform(
                        run_chat_history_request(
                            state.daemon_url.clone(),
                            state.token.clone(),
                            state.user_id.clone(),
                            60,
                        ),
                        Message::HistoryLoaded,
                    );
                }
            } else if !state.daemon_starting {
                state.daemon_status = "Daemon not reachable".to_string();
            }
            Task::none()
        }
        Message::StartDaemonPressed => {
            state.daemon_starting = true;
            state.daemon_status = "Starting daemon...".to_string();
            state.push_activity("daemon start requested".to_string());
            if state.manage_local_daemon {
                return Task::perform(
                    start_local_daemon(
                        state.daemon_url.clone(),
                        state.db_path.clone(),
                        state.token.clone(),
                    ),
                    Message::DaemonStartFinished,
                );
            }
            Task::perform(
                check_external_start_status(state.daemon_url.clone()),
                Message::DaemonStartFinished,
            )
        }
        Message::StopDaemonPressed => {
            state.daemon_starting = true;
            if state.manage_local_daemon {
                return Task::perform(stop_local_daemon(), Message::DaemonStopFinished);
            }
            state.daemon_starting = false;
            state.daemon_status = "External mode: stop daemon from service/process manager".to_string();
            Task::none()
        }
        Message::DaemonStartFinished(result) => {
            state.daemon_starting = false;
            match result {
                Ok(status) => {
                    state.daemon_status = status.clone();
                    state.push_activity(status);
                    Task::perform(
                        check_daemon_health(state.daemon_url.clone()),
                        Message::HealthChecked,
                    )
                }
                Err(err) => {
                    state.daemon_running = false;
                    state.daemon_status = err.clone();
                    state.push_activity(format!("daemon start failed: {err}"));
                    Task::none()
                }
            }
        }
        Message::DaemonStopFinished(result) => {
            state.daemon_starting = false;
            match result {
                Ok(status) => {
                    state.daemon_running = false;
                    state.daemon_status = status.clone();
                    state.push_activity(status);
                }
                Err(err) => {
                    state.daemon_status = err.clone();
                    state.push_activity(format!("daemon stop failed: {err}"));
                }
            }
            Task::none()
        }
        Message::LoadHistoryPressed => Task::perform(
            run_chat_history_request(
                state.daemon_url.clone(),
                state.token.clone(),
                state.user_id.clone(),
                60,
            ),
            Message::HistoryLoaded,
        ),
        Message::HistoryLoaded(result) => {
            match result {
                Ok(lines) => {
                    state.chat_messages.clear();
                    for line in lines {
                        if let Some((role, text, ts)) = parse_history_entry(&line) {
                            state.chat_messages.push(ChatMessage {
                                role,
                                text,
                                timestamp: ts.unwrap_or_else(now_unix_ts),
                            });
                            state.next_id = state.next_id.saturating_add(1);
                        }
                    }
                    state.push_activity("chat history loaded".to_string());
                }
                Err(err) => {
                    state.error = format!("History load failed: {err}");
                }
            }
            Task::none()
        }
        Message::ClearHistoryPressed => Task::perform(
            run_clear_user_history_request(
                state.daemon_url.clone(),
                state.token.clone(),
                state.user_id.clone(),
            ),
            Message::HistoryCleared,
        ),
        Message::HistoryCleared(result) => {
            match result {
                Ok(()) => {
                    state.chat_messages.clear();
                    state.push_activity("chat history cleared".to_string());
                }
                Err(err) => state.error = format!("Clear history failed: {err}"),
            }
            Task::none()
        }
        Message::LoadSettingsPressed => {
            Task::perform(load_settings(state.db_path.clone()), Message::SettingsLoaded)
        }
        Message::SettingsLoaded(result) => {
            match result {
                Ok(loaded) => {
                    state.settings = loaded.form;
                    state.settings_json = loaded.pretty_json;
                    state.settings_status = "Settings loaded".to_string();
                    state.settings_error.clear();
                }
                Err(err) => {
                    state.settings_error = err;
                    state.settings_status.clear();
                }
            }
            Task::none()
        }
        Message::SaveSettingsPressed => Task::perform(
            save_settings(
                state.db_path.clone(),
                state.settings.clone(),
                state.daemon_url.clone(),
                state.token.clone(),
            ),
            Message::SettingsSaved,
        ),
        Message::SettingsSaved(result) => {
            match result {
                Ok(status) => {
                    state.settings_status = status.clone();
                    state.settings_error.clear();
                    state.push_activity(status);
                }
                Err(err) => {
                    state.settings_error = err.clone();
                    state.settings_status.clear();
                    state.push_activity(format!("settings save failed: {err}"));
                }
            }
            Task::none()
        }
        Message::GithubPatChanged(value) => {
            state.settings.github_pat = value;
            Task::none()
        }
        Message::ZapierTokenChanged(value) => {
            state.settings.zapier_token = value;
            Task::none()
        }
        Message::OpenAiKeyChanged(value) => {
            state.settings.openai_api_key = value;
            Task::none()
        }
        Message::SearchKeyChanged(value) => {
            state.settings.search_api_key = value;
            Task::none()
        }
        Message::OpenAiBaseUrlChanged(value) => {
            state.settings.openai_base_url = value;
            Task::none()
        }
        Message::OpenAiModelChanged(value) => {
            state.settings.openai_model = value;
            Task::none()
        }
        Message::PromptTextChanged(value) => {
            state.settings.prompt_text = value;
            Task::none()
        }
        Message::HeartbeatTextChanged(value) => {
            state.settings.heartbeat_text = value;
            Task::none()
        }
        Message::MemoryEnabledChanged(value) => {
            state.settings.memory_enabled = value;
            Task::none()
        }
        Message::RunDoctorPressed => {
            if !state.daemon_running {
                state.doctor_error = "Daemon is not running".to_string();
                return Task::none();
            }
            state.doctor_status = "Running diagnostics...".to_string();
            state.doctor_error.clear();
            Task::perform(
                run_doctor_request(state.daemon_url.clone(), state.token.clone()),
                Message::DoctorFinished,
            )
        }
        Message::DoctorFinished(result) => {
            match result {
                Ok(report) => {
                    state.doctor_overall = report.overall.clone();
                    state.doctor_checks = report.checks;
                    state.doctor_status = format!("Doctor complete ({})", state.doctor_overall);
                    state.push_activity(state.doctor_status.clone());
                }
                Err(err) => {
                    state.doctor_error = err.clone();
                    state.doctor_status.clear();
                    state.push_activity(format!("doctor failed: {err}"));
                }
            }
            Task::none()
        }
        Message::RunSecurityPressed => {
            if !state.daemon_running {
                state.security_error = "Daemon is not running".to_string();
                return Task::none();
            }
            state.security_status = "Running security audit...".to_string();
            state.security_error.clear();
            Task::perform(
                run_security_audit_request(state.daemon_url.clone(), state.token.clone()),
                Message::SecurityFinished,
            )
        }
        Message::SecurityFinished(result) => {
            match result {
                Ok(report) => {
                    state.security_overall = report.overall.clone();
                    state.security_findings = report.findings;
                    state.security_status =
                        format!("Security audit complete ({})", state.security_overall);
                    state.push_activity(state.security_status.clone());
                }
                Err(err) => {
                    state.security_error = err.clone();
                    state.security_status.clear();
                    state.push_activity(format!("security audit failed: {err}"));
                }
            }
            Task::none()
        }
    }
}

fn view(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let tabs = UiTab::all().into_iter().fold(row!().spacing(8), |row, tab| {
        row.push(button(tab.label()).on_press(Message::TabSelected(tab)))
    });

    let daemon_controls = row![
        text(format!("daemon: {}", state.daemon_url)).size(14),
        button(if state.daemon_starting {
            "Starting..."
        } else {
            "Start"
        })
        .on_press_maybe((!state.daemon_starting).then_some(Message::StartDaemonPressed)),
        button("Stop").on_press(Message::StopDaemonPressed),
        text(if state.daemon_running {
            "healthy"
        } else {
            "offline"
        })
        .size(14),
        text(state.daemon_status.clone()).size(14),
    ]
    .spacing(10);

    let body = match state.active_tab {
        UiTab::Chat => view_chat_tab(state),
        UiTab::Activity => view_activity_tab(state),
        UiTab::Settings => view_settings_tab(state),
        UiTab::Diagnostics => view_diagnostics_tab(state),
    };

    let content = column![
        row![text("Butterfly Bot (Iced)").size(24), text(format!("user: {}", state.user_id)).size(14)]
            .spacing(16),
        tabs,
        daemon_controls,
        body
    ]
    .spacing(10)
    .padding(12)
    .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_chat_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let list = state
        .chat_messages
        .iter()
        .fold(column!().spacing(8), |col, msg| {
            let who = match msg.role {
                MessageRole::User => "You",
                MessageRole::Bot => "Bot",
                MessageRole::System => "System",
            };
            col.push(text(format!(
                "[{}] {}: {}",
                format_local_time(msg.timestamp),
                who,
                msg.text
            )))
        });

    let history_controls = row![
        button("Load history").on_press(Message::LoadHistoryPressed),
        button("Clear history").on_press(Message::ClearHistoryPressed)
    ]
    .spacing(10);

    let composer = row![
        text_input("Type a message", &state.composer)
            .on_input(Message::ComposerChanged)
            .on_submit(Message::SendPressed)
            .padding(10)
            .width(Length::Fill),
        button(if state.busy { "Sending..." } else { "Send" })
            .on_press_maybe((!state.busy).then_some(Message::SendPressed)),
    ]
    .spacing(10);

    column![
        history_controls,
        scrollable(list).height(Length::Fill),
        if state.error.is_empty() {
            text("")
        } else {
            text(state.error.clone())
        },
        composer
    ]
    .spacing(10)
    .height(Length::Fill)
    .into()
}

fn view_activity_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let list = state
        .activity_messages
        .iter()
        .fold(column!().spacing(8), |col, msg| {
            col.push(text(format!(
                "[{}] {}",
                format_local_time(msg.timestamp),
                msg.text
            )))
        });

    scrollable(list).height(Length::Fill).into()
}

fn view_settings_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let form = column![
        row![
            button("Reload").on_press(Message::LoadSettingsPressed),
            button("Save").on_press(Message::SaveSettingsPressed)
        ]
        .spacing(10),
        text_input("GitHub PAT", &state.settings.github_pat)
            .on_input(Message::GithubPatChanged)
            .padding(8),
        text_input("Zapier token", &state.settings.zapier_token)
            .on_input(Message::ZapierTokenChanged)
            .padding(8),
        text_input("OpenAI API key", &state.settings.openai_api_key)
            .on_input(Message::OpenAiKeyChanged)
            .padding(8),
        text_input("Search API key (Grok)", &state.settings.search_api_key)
            .on_input(Message::SearchKeyChanged)
            .padding(8),
        text_input("OpenAI base URL", &state.settings.openai_base_url)
            .on_input(Message::OpenAiBaseUrlChanged)
            .padding(8),
        text_input("OpenAI model", &state.settings.openai_model)
            .on_input(Message::OpenAiModelChanged)
            .padding(8),
        checkbox(state.settings.memory_enabled)
            .label("Memory enabled")
            .on_toggle(Message::MemoryEnabledChanged),
        text_input("Prompt markdown or URL", &state.settings.prompt_text)
            .on_input(Message::PromptTextChanged)
            .padding(8),
        text_input("Heartbeat markdown or URL", &state.settings.heartbeat_text)
            .on_input(Message::HeartbeatTextChanged)
            .padding(8),
        text("Config preview:").size(14),
        scrollable(text(state.settings_json.clone())).height(180),
        if state.settings_error.is_empty() {
            text(state.settings_status.clone())
        } else {
            text(state.settings_error.clone())
        }
    ]
    .spacing(8);

    scrollable(form).height(Length::Fill).into()
}

fn view_diagnostics_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let doctor_lines = state
        .doctor_checks
        .iter()
        .fold(column!().spacing(6), |col, check| {
            let mut line = format!("{} [{}] — {}", check.name, check.status, check.message);
            if let Some(hint) = &check.fix_hint {
                line.push_str(&format!(" ({hint})"));
            }
            col.push(text(line))
        });

    let security_lines = state
        .security_findings
        .iter()
        .fold(column!().spacing(6), |col, finding| {
            let mut line = format!(
                "{} [{} / {}] — {}{}",
                finding.id,
                finding.severity,
                finding.status,
                finding.message,
                if finding.auto_fixable { " (auto-fixable)" } else { "" }
            );
            if let Some(hint) = &finding.fix_hint {
                line.push_str(&format!(" ({hint})"));
            }
            col.push(text(line))
        });

    let content = column![
        row![
            button("Run doctor").on_press(Message::RunDoctorPressed),
            button("Run security audit").on_press(Message::RunSecurityPressed)
        ]
        .spacing(10),
        text(state.doctor_status.clone()),
        if state.doctor_error.is_empty() {
            text("")
        } else {
            text(state.doctor_error.clone())
        },
        text(format!("Doctor overall: {}", state.doctor_overall)),
        doctor_lines,
        text(""),
        text(state.security_status.clone()),
        if state.security_error.is_empty() {
            text("")
        } else {
            text(state.security_error.clone())
        },
        text(format!("Security overall: {}", state.security_overall)),
        security_lines,
    ]
    .spacing(8);

    scrollable(content).height(Length::Fill).into()
}

async fn send_prompt(
    daemon_url: String,
    user_id: String,
    token: String,
    prompt: String,
) -> Result<String, String> {
    let client = daemon_request_client();
    let url = format!("{}/process_text", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url).json(&ProcessTextRequest {
        user_id,
        text: prompt,
        prompt: None,
    });
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }

    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {body}"));
    }

    let json = serde_json::from_str::<serde_json::Value>(&body).ok();
    if let Some(text) = json
        .as_ref()
        .and_then(|v| v.get("response"))
        .and_then(|v| v.as_str())
    {
        return Ok(text.to_string());
    }
    if let Some(text) = json
        .as_ref()
        .and_then(|v| v.get("output"))
        .and_then(|v| v.as_str())
    {
        return Ok(text.to_string());
    }

    Ok(body)
}

fn daemon_request_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("request client")
}

async fn check_daemon_health(daemon_url: String) -> bool {
    let client = daemon_request_client();
    let url = format!("{}/health", daemon_url.trim_end_matches('/'));
    match client.get(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

async fn run_doctor_request(daemon_url: String, token: String) -> Result<DoctorResponse, String> {
    let client = daemon_request_client();
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

async fn run_security_audit_request(
    daemon_url: String,
    token: String,
) -> Result<SecurityAuditResponse, String> {
    let client = daemon_request_client();
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

async fn run_chat_history_request(
    daemon_url: String,
    token: String,
    user_id: String,
    limit: usize,
) -> Result<Vec<String>, String> {
    let client = daemon_request_client();
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
    let client = daemon_request_client();
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

fn parse_history_entry(line: &str) -> Option<(MessageRole, String, Option<i64>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (role, rest) = if let Some(value) = trimmed.strip_prefix("user:") {
        (MessageRole::User, value.trim())
    } else if let Some(value) = trimmed.strip_prefix("assistant:") {
        (MessageRole::Bot, value.trim())
    } else {
        (MessageRole::System, trimmed)
    };

    if let Some((ts_text, body)) = rest.split_once(' ') {
        if let Ok(ts) = ts_text.parse::<i64>() {
            return Some((role, body.trim().to_string(), Some(ts)));
        }
    }

    Some((role, rest.to_string(), None))
}

fn now_unix_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn format_local_time(ts: i64) -> String {
    let seconds_in_day = 24 * 60 * 60;
    let mut secs = (ts % seconds_in_day) as i32;
    if secs < 0 {
        secs += seconds_in_day as i32;
    }
    let hour = secs / 3600;
    let minute = (secs % 3600) / 60;
    let second = secs % 60;
    format!(
        "{:02}:{:02}:{:02}",
        hour,
        minute,
        second
    )
}

async fn check_external_start_status(daemon_url: String) -> Result<String, String> {
    if check_daemon_health(daemon_url).await {
        Ok("External daemon healthy".to_string())
    } else {
        Err("External daemon not reachable".to_string())
    }
}

fn env_flag_enabled(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => default,
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

struct DaemonControl {
    child: Child,
}

fn daemon_control() -> &'static Mutex<Option<DaemonControl>> {
    static CONTROL: OnceLock<Mutex<Option<DaemonControl>>> = OnceLock::new();
    CONTROL.get_or_init(|| Mutex::new(None))
}

fn daemon_binary_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            candidates.push(dir.join("butterfly-botd"));
            if let Some(parent) = dir.parent() {
                candidates.push(parent.join("butterfly-botd"));
            }
        }
    }
    candidates.push(PathBuf::from("butterfly-botd"));
    candidates
}

async fn start_local_daemon(daemon_url: String, db_path: String, token: String) -> Result<String, String> {
    let (host, port) = parse_daemon_address(&daemon_url);
    let mut selected = None;
    for candidate in daemon_binary_candidates() {
        if candidate.exists() || candidate == PathBuf::from("butterfly-botd") {
            selected = Some(candidate);
            break;
        }
    }
    let Some(binary) = selected else {
        return Err("Could not find butterfly-botd binary".to_string());
    };

    let mut command = Command::new(binary);
    command
        .arg("--host")
        .arg(&host)
        .arg("--port")
        .arg(port.to_string())
        .env("BUTTERFLY_BOT_DB", db_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !token.trim().is_empty() {
        command.env("BUTTERFLY_BOT_TOKEN", token);
    }

    let child = command
        .spawn()
        .map_err(|err| format!("Failed to spawn daemon: {err}"))?;
    if let Ok(mut control) = daemon_control().lock() {
        *control = Some(DaemonControl { child });
    }

    let client = daemon_request_client();
    let health_url = format!("{}/health", daemon_url.trim_end_matches('/'));
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                return Ok("Daemon started".to_string());
            }
        }
    }
    Err("Daemon start timed out".to_string())
}

async fn stop_local_daemon() -> Result<String, String> {
    let mut control = daemon_control()
        .lock()
        .map_err(|_| "Failed to lock daemon control".to_string())?;
    if let Some(mut daemon) = control.take() {
        daemon
            .child
            .kill()
            .map_err(|err| format!("Failed to stop daemon: {err}"))?;
        let _ = daemon.child.wait();
        Ok("Daemon stopped".to_string())
    } else {
        Ok("No local daemon process to stop".to_string())
    }
}

fn markdown_source_from_input(value: &str) -> crate::config::MarkdownSource {
    let trimmed = value.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        crate::config::MarkdownSource::Url {
            url: trimmed.to_string(),
        }
    } else {
        crate::config::MarkdownSource::Database {
            markdown: value.to_string(),
        }
    }
}

async fn load_settings(db_path: String) -> Result<LoadedSettings, String> {
    tokio::task::spawn_blocking(move || {
        let config = crate::config::Config::from_store(&db_path)
            .map_err(|err| format!("Failed to load config: {err}"))?;

        let github_pat = crate::vault::get_secret_required("github_pat")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let zapier_token = crate::vault::get_secret_required("zapier_token")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let openai_api_key = crate::vault::get_secret_required("openai_api_key")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let search_api_key = crate::vault::get_secret_required("search_internet_grok_api_key")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();

        let openai_base_url = config
            .openai
            .as_ref()
            .and_then(|v| v.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let openai_model = config
            .openai
            .as_ref()
            .and_then(|v| v.model.clone())
            .unwrap_or_else(|| "gpt-4.1-mini".to_string());
        let memory_enabled = config
            .memory
            .as_ref()
            .and_then(|v| v.enabled)
            .unwrap_or(true);

        let prompt_text = match &config.prompt_source {
            crate::config::MarkdownSource::Url { url } => url.clone(),
            crate::config::MarkdownSource::Database { markdown } => markdown.clone(),
        };
        let heartbeat_text = match &config.heartbeat_source {
            crate::config::MarkdownSource::Url { url } => url.clone(),
            crate::config::MarkdownSource::Database { markdown } => markdown.clone(),
        };

        let pretty_json = serde_json::to_string_pretty(&config)
            .map_err(|err| format!("Failed to serialize config: {err}"))?;

        Ok(LoadedSettings {
            form: SettingsForm {
                github_pat,
                zapier_token,
                openai_api_key,
                search_api_key,
                openai_base_url,
                openai_model,
                prompt_text,
                heartbeat_text,
                memory_enabled,
            },
            pretty_json,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

async fn save_settings(
    db_path: String,
    form: SettingsForm,
    daemon_url: String,
    token: String,
) -> Result<String, String> {
    let config = tokio::task::spawn_blocking({
        let db_path = db_path.clone();
        let form = form.clone();
        move || {
            let mut config = crate::config::Config::from_store(&db_path)
                .map_err(|err| format!("Failed to load config: {err}"))?;

            let openai = config
                .openai
                .get_or_insert(crate::config::OpenAiConfig {
                    api_key: None,
                    model: None,
                    base_url: None,
                });
            openai.base_url = Some(form.openai_base_url.clone());
            openai.model = Some(form.openai_model.clone());
            openai.api_key = None;

            let memory = config
                .memory
                .get_or_insert(crate::config::MemoryConfig {
                    enabled: Some(form.memory_enabled),
                    sqlite_path: Some(db_path.clone()),
                    summary_model: None,
                    embedding_model: None,
                    rerank_model: None,
                    openai: None,
                    context_embed_enabled: Some(false),
                    summary_threshold: None,
                    retention_days: None,
                });
            memory.enabled = Some(form.memory_enabled);

            config.prompt_source = markdown_source_from_input(&form.prompt_text);
            config.heartbeat_source = markdown_source_from_input(&form.heartbeat_text);

            crate::vault::set_secret_required("github_pat", &form.github_pat)
                .map_err(|err| format!("Failed to store GitHub token: {err}"))?;
            crate::vault::set_secret_required("zapier_token", &form.zapier_token)
                .map_err(|err| format!("Failed to store Zapier token: {err}"))?;
            crate::vault::set_secret_required("openai_api_key", &form.openai_api_key)
                .map_err(|err| format!("Failed to store OpenAI API key: {err}"))?;
            crate::vault::set_secret_required("coding_openai_api_key", &form.openai_api_key)
                .map_err(|err| format!("Failed to store coding API key: {err}"))?;
            crate::vault::set_secret_required("search_internet_grok_api_key", &form.search_api_key)
                .map_err(|err| format!("Failed to store search API key: {err}"))?;

            crate::config_store::save_config(&db_path, &config)
                .map_err(|err| format!("Failed to save config: {err}"))?;

            Ok::<crate::config::Config, String>(config)
        }
    })
    .await
    .map_err(|err| err.to_string())??;

    let pretty = serde_json::to_string_pretty(&config).map_err(|err| err.to_string())?;
    let _ = tokio::task::spawn_blocking(move || crate::vault::set_secret_required("app_config_json", &pretty)).await;

    let client = daemon_request_client();
    let url = format!("{}/reload_config", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let _ = request.send().await;

    Ok("Settings saved".to_string())
}
