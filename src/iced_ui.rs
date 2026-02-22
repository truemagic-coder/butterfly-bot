use ::time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};
use iced::widget::{
    button, column, container, image, markdown, row, scrollable, text, text_editor, text_input,
    Space,
};
use iced::{
    application, time, Background, Border, Color, Element, Length, Shadow, Size, Subscription,
    Task, Theme,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const BUTTERFLY_BOT_LOGO_BYTES: &[u8] =
    include_bytes!("../assets/icons/hicolor/512x512/apps/butterfly-bot.png");

fn butterfly_bot_logo_handle() -> image::Handle {
    static HANDLE: OnceLock<image::Handle> = OnceLock::new();
    HANDLE
        .get_or_init(|| image::Handle::from_bytes(BUTTERFLY_BOT_LOGO_BYTES))
        .clone()
}

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

#[derive(Clone, Debug, Deserialize)]
struct SolanaWalletUiResponse {
    address: String,
}

#[derive(Clone)]
struct ChatMessage {
    role: MessageRole,
    text: String,
    markdown_items: Vec<markdown::Item>,
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
    Context,
    Heartbeat,
}

impl UiTab {
    fn all() -> [UiTab; 6] {
        [
            UiTab::Chat,
            UiTab::Activity,
            UiTab::Settings,
            UiTab::Diagnostics,
            UiTab::Context,
            UiTab::Heartbeat,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            UiTab::Chat => "Chat",
            UiTab::Activity => "Activity",
            UiTab::Settings => "Config",
            UiTab::Diagnostics => "Diagnostics",
            UiTab::Context => "Context",
            UiTab::Heartbeat => "Heartbeat",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct UiServerRow {
    name: String,
    url: String,
    header_key: String,
    header_value: String,
}

#[derive(Clone, Debug)]
struct SettingsForm {
    github_pat: String,
    zapier_token: String,
    openai_api_key: String,
    grok_api_key: String,
    solana_rpc_endpoint: String,
    search_network_allow: String,
    wakeup_poll_seconds: String,
    tpm_mode: String,
    mcp_servers: Vec<UiServerRow>,
    http_call_servers: Vec<UiServerRow>,
    prompt_text: String,
    heartbeat_text: String,
}

impl Default for SettingsForm {
    fn default() -> Self {
        Self {
            github_pat: String::new(),
            zapier_token: String::new(),
            openai_api_key: String::new(),
            grok_api_key: String::new(),
            solana_rpc_endpoint: String::new(),
            search_network_allow: String::new(),
            wakeup_poll_seconds: "60".to_string(),
            tpm_mode: "auto".to_string(),
            mcp_servers: vec![],
            http_call_servers: vec![],
            prompt_text: String::new(),
            heartbeat_text: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct LoadedSettings {
    form: SettingsForm,
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
    solana_wallet_address: Option<String>,
    solana_wallet_status: String,
    solana_wallet_fetch_in_flight: bool,
    solana_wallet_refresh_pending: bool,
    context_preview_items: Vec<markdown::Item>,
    heartbeat_preview_items: Vec<markdown::Item>,
    context_editor: text_editor::Content,
    heartbeat_editor: text_editor::Content,
    manage_local_daemon: bool,
    daemon_autostart_attempted: bool,
}

#[derive(Clone, Debug)]
struct DaemonHealth {
    daemon_url: String,
    healthy: bool,
    switched: bool,
}

#[derive(Clone, Debug)]
enum Message {
    Tick,
    TabSelected(UiTab),
    ComposerChanged(String),
    SendPressed,
    ResponseReady(Result<String, String>),
    HealthChecked(DaemonHealth),
    StartDaemonPressed,
    StopDaemonPressed,
    DaemonStartFinished(Result<String, String>),
    DaemonStopFinished(Result<String, String>),
    HistoryLoaded(Result<Vec<String>, String>),
    ClearHistoryPressed,
    HistoryCleared(Result<(), String>),
    LoadSettingsPressed,
    SettingsLoaded(Box<Result<LoadedSettings, String>>),
    SaveSettingsPressed,
    SettingsSaved(Result<String, String>),
    GithubPatChanged(String),
    ZapierTokenChanged(String),
    OpenAiKeyChanged(String),
    GrokKeyChanged(String),
    SolanaRpcEndpointChanged(String),
    SearchNetworkAllowChanged(String),
    WakeupPollSecondsChanged(String),
    TpmModeChanged(String),
    AddMcpServer,
    RemoveMcpServer(usize),
    McpServerNameChanged(usize, String),
    McpServerUrlChanged(usize, String),
    McpServerHeaderKeyChanged(usize, String),
    McpServerHeaderValueChanged(usize, String),
    AddHttpServer,
    RemoveHttpServer(usize),
    HttpServerNameChanged(usize, String),
    HttpServerUrlChanged(usize, String),
    HttpServerHeaderKeyChanged(usize, String),
    HttpServerHeaderValueChanged(usize, String),
    MarkdownLinkClicked(String),
    ContextEdited(text_editor::Action),
    HeartbeatEdited(text_editor::Action),
    RunDoctorPressed,
    DoctorFinished(Result<DoctorResponse, String>),
    SecurityFinished(Result<SecurityAuditResponse, String>),
    RefreshSolanaWallet,
    SolanaWalletLoaded(Result<Option<String>, String>),
    CopyToClipboard(String),
}

pub fn launch_ui(config: IcedUiLaunchConfig) -> iced::Result {
    if env_flag_enabled("BUTTERFLY_UI_MANAGE_DAEMON", true) {
        kill_all_daemons_best_effort();
    }

    let boot_config = config.clone();
    application(
        move || {
            let state = ButterflyIcedApp::new(boot_config.clone());
            let boot = Task::batch(vec![
                Task::perform(
                    check_daemon_health(state.daemon_url.clone()),
                    Message::HealthChecked,
                ),
                Task::perform(load_settings(state.db_path.clone()), |result| {
                    Message::SettingsLoaded(Box::new(result))
                }),
            ]);
            (state, boot)
        },
        update,
        view,
    )
    .title(app_title)
    .theme(app_theme)
    .window(iced::window::Settings {
        size: Size::new(1280.0, 860.0),
        min_size: Some(Size::new(980.0, 700.0)),
        ..Default::default()
    })
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
        let manage_local_daemon = env_flag_enabled("BUTTERFLY_UI_MANAGE_DAEMON", true);
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
            solana_wallet_address: None,
            solana_wallet_status: String::new(),
            solana_wallet_fetch_in_flight: false,
            solana_wallet_refresh_pending: true,
            context_preview_items: vec![],
            heartbeat_preview_items: vec![],
            context_editor: text_editor::Content::new(),
            heartbeat_editor: text_editor::Content::new(),
            manage_local_daemon,
            daemon_autostart_attempted: false,
        }
    }

    fn push_chat(&mut self, role: MessageRole, text: String) {
        let markdown_items = parse_markdown_items(&text);
        self.chat_messages.push(ChatMessage {
            role,
            text,
            markdown_items,
            timestamp: now_unix_ts(),
        });
        if self.chat_messages.len() > 300 {
            let drop_count = self.chat_messages.len() - 300;
            self.chat_messages.drain(0..drop_count);
        }
    }

    fn push_activity(&mut self, text: String) {
        let markdown_items = parse_markdown_items(&text);
        self.activity_messages.push(ChatMessage {
            role: MessageRole::System,
            text,
            markdown_items,
            timestamp: now_unix_ts(),
        });
        if self.activity_messages.len() > 300 {
            let drop_count = self.activity_messages.len() - 300;
            self.activity_messages.drain(0..drop_count);
        }
    }
}

impl Drop for ButterflyIcedApp {
    fn drop(&mut self) {
        if self.manage_local_daemon {
            let _ = stop_local_daemon_blocking();
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

            Task::perform(
                send_prompt(daemon_url, user_id, token, prompt),
                Message::ResponseReady,
            )
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
        Message::HealthChecked(health) => {
            if health.switched && state.daemon_url != health.daemon_url {
                state.daemon_url = health.daemon_url.clone();
                state.push_activity(format!("daemon auto-detected on {}", state.daemon_url));
            }

            state.daemon_running = health.healthy;
            if health.healthy {
                if state.daemon_status.is_empty() || state.daemon_status.contains("not reachable") {
                    state.daemon_status = "Daemon healthy".to_string();
                }
                if state.solana_wallet_refresh_pending && !state.solana_wallet_fetch_in_flight {
                    state.solana_wallet_fetch_in_flight = true;
                    return Task::perform(
                        fetch_solana_wallet_address(
                            state.daemon_url.clone(),
                            state.token.clone(),
                            state.user_id.clone(),
                        ),
                        Message::SolanaWalletLoaded,
                    );
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
            } else if state.manage_local_daemon
                && !state.daemon_autostart_attempted
                && !state.daemon_starting
            {
                state.daemon_autostart_attempted = true;
                state.daemon_starting = true;
                state.daemon_status = "Starting daemon...".to_string();
                state.push_activity("daemon auto-start requested".to_string());
                return Task::perform(
                    start_local_daemon(
                        state.daemon_url.clone(),
                        state.db_path.clone(),
                        state.token.clone(),
                    ),
                    Message::DaemonStartFinished,
                );
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
                return Task::perform(
                    stop_daemon_by_url(state.daemon_url.clone()),
                    Message::DaemonStopFinished,
                );
            }
            state.daemon_starting = false;
            state.daemon_status =
                "External mode: stop daemon from service/process manager".to_string();
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
        Message::HistoryLoaded(result) => {
            match result {
                Ok(lines) => {
                    state.chat_messages.clear();
                    for line in lines {
                        if let Some((role, text, ts)) = parse_history_entry(&line) {
                            let markdown_items = parse_markdown_items(&text);
                            state.chat_messages.push(ChatMessage {
                                role,
                                text,
                                markdown_items,
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
            Task::perform(load_settings(state.db_path.clone()), |result| {
                Message::SettingsLoaded(Box::new(result))
            })
        }
        Message::SettingsLoaded(result) => {
            match *result {
                Ok(loaded) => {
                    state.settings = loaded.form;
                    state.context_editor =
                        text_editor::Content::with_text(&state.settings.prompt_text);
                    state.heartbeat_editor =
                        text_editor::Content::with_text(&state.settings.heartbeat_text);
                    state.context_preview_items = parse_markdown_items(&state.settings.prompt_text);
                    state.heartbeat_preview_items =
                        parse_markdown_items(&state.settings.heartbeat_text);
                    state.settings_status = "Settings loaded".to_string();
                    state.settings_error.clear();
                    state.solana_wallet_refresh_pending = true;
                    if state.daemon_running && !state.solana_wallet_fetch_in_flight {
                        state.solana_wallet_fetch_in_flight = true;
                        return Task::perform(
                            fetch_solana_wallet_address(
                                state.daemon_url.clone(),
                                state.token.clone(),
                                state.user_id.clone(),
                            ),
                            Message::SolanaWalletLoaded,
                        );
                    }
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
                    state.solana_wallet_refresh_pending = true;
                    if state.daemon_running && !state.solana_wallet_fetch_in_flight {
                        state.solana_wallet_fetch_in_flight = true;
                        return Task::perform(
                            fetch_solana_wallet_address(
                                state.daemon_url.clone(),
                                state.token.clone(),
                                state.user_id.clone(),
                            ),
                            Message::SolanaWalletLoaded,
                        );
                    }
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
        Message::GrokKeyChanged(value) => {
            state.settings.grok_api_key = value;
            Task::none()
        }
        Message::SolanaRpcEndpointChanged(value) => {
            state.settings.solana_rpc_endpoint = value;
            Task::none()
        }
        Message::SearchNetworkAllowChanged(value) => {
            state.settings.search_network_allow = value;
            Task::none()
        }
        Message::WakeupPollSecondsChanged(value) => {
            state.settings.wakeup_poll_seconds = value;
            Task::none()
        }
        Message::TpmModeChanged(value) => {
            state.settings.tpm_mode = value;
            Task::none()
        }
        Message::AddMcpServer => {
            state.settings.mcp_servers.push(UiServerRow::default());
            Task::none()
        }
        Message::RemoveMcpServer(index) => {
            if index < state.settings.mcp_servers.len() {
                state.settings.mcp_servers.remove(index);
            }
            Task::none()
        }
        Message::McpServerNameChanged(index, value) => {
            if let Some(row) = state.settings.mcp_servers.get_mut(index) {
                row.name = value;
            }
            Task::none()
        }
        Message::McpServerUrlChanged(index, value) => {
            if let Some(row) = state.settings.mcp_servers.get_mut(index) {
                row.url = value;
            }
            Task::none()
        }
        Message::McpServerHeaderKeyChanged(index, value) => {
            if let Some(row) = state.settings.mcp_servers.get_mut(index) {
                row.header_key = value;
            }
            Task::none()
        }
        Message::McpServerHeaderValueChanged(index, value) => {
            if let Some(row) = state.settings.mcp_servers.get_mut(index) {
                row.header_value = value;
            }
            Task::none()
        }
        Message::AddHttpServer => {
            state
                .settings
                .http_call_servers
                .push(UiServerRow::default());
            Task::none()
        }
        Message::RemoveHttpServer(index) => {
            if index < state.settings.http_call_servers.len() {
                state.settings.http_call_servers.remove(index);
            }
            Task::none()
        }
        Message::HttpServerNameChanged(index, value) => {
            if let Some(row) = state.settings.http_call_servers.get_mut(index) {
                row.name = value;
            }
            Task::none()
        }
        Message::HttpServerUrlChanged(index, value) => {
            if let Some(row) = state.settings.http_call_servers.get_mut(index) {
                row.url = value;
            }
            Task::none()
        }
        Message::HttpServerHeaderKeyChanged(index, value) => {
            if let Some(row) = state.settings.http_call_servers.get_mut(index) {
                row.header_key = value;
            }
            Task::none()
        }
        Message::HttpServerHeaderValueChanged(index, value) => {
            if let Some(row) = state.settings.http_call_servers.get_mut(index) {
                row.header_value = value;
            }
            Task::none()
        }
        Message::MarkdownLinkClicked(uri) => {
            state.push_activity(format!("link clicked: {uri}"));
            let _ = open_uri_best_effort(&uri);
            Task::none()
        }
        Message::ContextEdited(action) => {
            state.context_editor.perform(action);
            state.settings.prompt_text = state.context_editor.text();
            state.context_preview_items = parse_markdown_items(&state.settings.prompt_text);
            Task::none()
        }
        Message::HeartbeatEdited(action) => {
            state.heartbeat_editor.perform(action);
            state.settings.heartbeat_text = state.heartbeat_editor.text();
            state.heartbeat_preview_items = parse_markdown_items(&state.settings.heartbeat_text);
            Task::none()
        }
        Message::RunDoctorPressed => {
            if !state.daemon_running {
                state.doctor_error = "Daemon is not running".to_string();
                state.security_error = "Daemon is not running".to_string();
                return Task::none();
            }
            state.doctor_status = "Running security doctor...".to_string();
            state.security_status = "Running security audit...".to_string();
            state.doctor_error.clear();
            state.security_error.clear();
            Task::batch(vec![
                Task::perform(
                    run_doctor_request(state.daemon_url.clone(), state.token.clone()),
                    Message::DoctorFinished,
                ),
                Task::perform(
                    run_security_audit_request(state.daemon_url.clone(), state.token.clone()),
                    Message::SecurityFinished,
                ),
            ])
        }
        Message::DoctorFinished(result) => {
            match result {
                Ok(report) => {
                    let reported_overall = report.overall;
                    state.doctor_checks = report
                        .checks
                        .into_iter()
                        .filter(|check| {
                            let name = check.name.trim().to_ascii_lowercase();
                            name != "provider_health" && name != "provider_check"
                        })
                        .collect();
                    state.doctor_overall = if state.doctor_checks.is_empty() {
                        reported_overall
                    } else {
                        derive_doctor_overall(&state.doctor_checks).to_string()
                    };
                    state.doctor_status = format!(
                        "Doctor complete ({})",
                        display_posture_level(&state.doctor_overall)
                    );
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
        Message::SecurityFinished(result) => {
            match result {
                Ok(report) => {
                    state.security_overall = report.overall.clone();
                    state.security_findings = report
                        .findings
                        .into_iter()
                        .filter(|finding| {
                            let id = finding.id.trim().to_ascii_lowercase();
                            id != "provider_health" && id != "provider_check"
                        })
                        .collect();
                    state.security_status = format!(
                        "Security audit complete ({})",
                        display_posture_level(&state.security_overall)
                    );
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
        Message::RefreshSolanaWallet => {
            state.solana_wallet_refresh_pending = true;
            if state.daemon_running && !state.solana_wallet_fetch_in_flight {
                state.solana_wallet_fetch_in_flight = true;
                Task::perform(
                    fetch_solana_wallet_address(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                    ),
                    Message::SolanaWalletLoaded,
                )
            } else {
                Task::none()
            }
        }
        Message::SolanaWalletLoaded(result) => {
            state.solana_wallet_fetch_in_flight = false;
            state.solana_wallet_refresh_pending = false;
            match result {
                Ok(Some(address)) => {
                    state.solana_wallet_address = Some(address);
                    state.solana_wallet_status = "Butterfly Bot Wallet detected".to_string();
                }
                Ok(None) => {
                    state.solana_wallet_address = None;
                    state.solana_wallet_status = "No Butterfly Bot Wallet available".to_string();
                }
                Err(err) => {
                    state.solana_wallet_address = None;
                    state.solana_wallet_status = format!("Solana wallet unavailable: {err}");
                }
            }
            Task::none()
        }
        Message::CopyToClipboard(value) => {
            state.push_activity("copied to clipboard".to_string());
            iced::clipboard::write(value)
        }
    }
}

fn view(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let tabs_row = UiTab::all()
        .into_iter()
        .fold(row!().spacing(8), |row, tab| {
            let active = tab == state.active_tab;
            row.push(
                button(text(tab.label()).size(14))
                    .padding([8, 14])
                    .style(if active {
                        iced::widget::button::primary
                    } else {
                        iced::widget::button::secondary
                    })
                    .on_press(Message::TabSelected(tab)),
            )
        });

    let daemon_controls = row![
        button(if state.daemon_starting { "⏳" } else { "▶" })
            .padding([8, 12])
            .on_press_maybe(
                (!state.daemon_starting && !state.daemon_running)
                    .then_some(Message::StartDaemonPressed)
            ),
        button("⏹").padding([8, 12]).on_press_maybe(
            (!state.daemon_starting && state.daemon_running).then_some(Message::StopDaemonPressed)
        ),
        button("🗑")
            .padding([8, 12])
            .style(iced::widget::button::danger)
            .on_press(Message::ClearHistoryPressed),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let nav_bar = row![
        container(
            scrollable(tabs_row).direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Scrollbar::default(),
            ))
        )
        .width(Length::Fill)
        .padding([2, 0]),
        daemon_controls,
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);

    let body = container(match state.active_tab {
        UiTab::Chat => view_chat_tab(state),
        UiTab::Activity => view_activity_tab(state),
        UiTab::Settings => view_settings_tab(state),
        UiTab::Diagnostics => view_diagnostics_tab(state),
        UiTab::Context => view_context_tab(state),
        UiTab::Heartbeat => view_heartbeat_tab(state),
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let logo: Element<'_, Message> = image::<image::Handle>(butterfly_bot_logo_handle())
        .width(40)
        .height(40)
        .into();

    let content = column![
        row![
            logo,
            column![
                text("Butterfly Bot").size(30),
                text("Personal-ops assistant").size(14)
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            text(state.daemon_status.clone()).size(14)
        ]
        .spacing(16)
        .width(Length::Fill)
        .align_y(iced::Alignment::Center),
        container(nav_bar).padding(8).style(glass_panel),
        body
    ]
    .spacing(12)
    .padding(16)
    .height(Length::Fill);

    container(container(content).height(Length::Fill).style(glass_shell))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

fn glass_shell(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba(0.07, 0.10, 0.18, 0.65))),
        border: Border {
            radius: 18.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(Color::from_rgba(0.10, 0.14, 0.24, 0.58))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_user_bubble(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(0.39, 0.40, 0.95, 0.62))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.14),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_bot_bubble(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(0.49, 0.23, 0.92, 0.55))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.14),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn view_chat_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let list = state
        .chat_messages
        .iter()
        .fold(column!().spacing(10).width(Length::Fill), |col, msg| {
            let who = match msg.role {
                MessageRole::User => "You",
                MessageRole::Bot => "Butterfly",
                MessageRole::System => "System",
            };
            let bubble = container(
                column![
                    row![
                        text(format!("{} • {}", who, format_local_time(msg.timestamp))).size(12),
                        Space::new().width(Length::Fill),
                        button(text("📋").size(14))
                            .padding(6)
                            .width(30)
                            .height(30)
                            .on_press(Message::CopyToClipboard(msg.text.clone()))
                    ]
                    .align_y(iced::Alignment::Center),
                    markdown::view(msg.markdown_items.iter(), markdown_render_settings())
                        .map(Message::MarkdownLinkClicked)
                ]
                .spacing(6),
            )
            .padding(12)
            .style(match msg.role {
                MessageRole::User => glass_user_bubble,
                MessageRole::Bot => glass_bot_bubble,
                MessageRole::System => glass_panel,
            });
            col.push(bubble)
        })
        .push(Space::new().height(18));

    let composer = row![
        text_input("Type a message", &state.composer)
            .on_input(Message::ComposerChanged)
            .on_submit(Message::SendPressed)
            .padding(12)
            .width(Length::Fill),
        button(if state.busy { "Sending..." } else { "Send" })
            .padding([10, 16])
            .style(iced::widget::button::primary)
            .on_press_maybe((!state.busy).then_some(Message::SendPressed)),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    column![
        container(
            scrollable(container(list).padding([0, 14]).width(Length::Fill))
                .height(Length::Fill)
                .width(Length::Fill)
                .anchor_bottom()
                .auto_scroll(true)
        )
        .padding(8)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill),
        if state.error.is_empty() {
            text("")
        } else {
            text(state.error.clone()).color([0.95, 0.45, 0.45])
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
            col.push(
                container(
                    row![
                        text(format!(
                            "[{}] {}",
                            format_local_time(msg.timestamp),
                            msg.text
                        )),
                        Space::new().width(Length::Fill),
                        button(text("📋").size(14))
                            .padding(6)
                            .width(30)
                            .height(30)
                            .on_press(Message::CopyToClipboard(msg.text.clone()))
                    ]
                    .align_y(iced::Alignment::Center),
                )
                .padding(10)
                .style(glass_panel),
            )
        });

    container(scrollable(list).height(Length::Fill).width(Length::Fill))
        .padding(8)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_settings_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let mcp_rows = state.settings.mcp_servers.iter().enumerate().fold(
        column!().spacing(8),
        |col, (index, server)| {
            col.push(
                column![
                    row![
                        text_input("Server name", &server.name)
                            .on_input(move |value| Message::McpServerNameChanged(index, value))
                            .padding(8)
                            .width(Length::FillPortion(2)),
                        text_input("https://server.example/mcp", &server.url)
                            .on_input(move |value| Message::McpServerUrlChanged(index, value))
                            .padding(8)
                            .width(Length::FillPortion(3)),
                        button("Remove")
                            .padding([8, 10])
                            .style(iced::widget::button::danger)
                            .on_press(Message::RemoveMcpServer(index)),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                    row![
                        text_input("Header key (optional)", &server.header_key)
                            .on_input(move |value| Message::McpServerHeaderKeyChanged(index, value))
                            .padding(8)
                            .width(Length::FillPortion(1)),
                        text_input("Header value (optional)", &server.header_value)
                            .on_input(move |value| Message::McpServerHeaderValueChanged(
                                index, value
                            ))
                            .padding(8)
                            .width(Length::FillPortion(1)),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                ]
                .spacing(8),
            )
        },
    );

    let http_rows = state.settings.http_call_servers.iter().enumerate().fold(
        column!().spacing(8),
        |col, (index, server)| {
            col.push(
                column![
                    row![
                        text_input("Server name", &server.name)
                            .on_input(move |value| Message::HttpServerNameChanged(index, value))
                            .padding(8)
                            .width(Length::FillPortion(2)),
                        text_input("https://api.example.com/v1", &server.url)
                            .on_input(move |value| Message::HttpServerUrlChanged(index, value))
                            .padding(8)
                            .width(Length::FillPortion(3)),
                        button("Remove")
                            .padding([8, 10])
                            .style(iced::widget::button::danger)
                            .on_press(Message::RemoveHttpServer(index)),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                    row![
                        text_input("Header key (optional)", &server.header_key)
                            .on_input(move |value| Message::HttpServerHeaderKeyChanged(
                                index, value
                            ))
                            .padding(8)
                            .width(Length::FillPortion(1)),
                        text_input("Header value (optional)", &server.header_value)
                            .on_input(move |value| Message::HttpServerHeaderValueChanged(
                                index, value
                            ))
                            .padding(8)
                            .width(Length::FillPortion(1)),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                ]
                .spacing(8),
            )
        },
    );

    let form = column![
            row![
                text("Provider: OpenAI (fixed)").size(14),
                Space::new().width(Length::Fill),
                button("Save")
                    .padding([8, 12])
                    .style(iced::widget::button::success)
                    .on_press(Message::SaveSettingsPressed)
            ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        container(column![
            text("API keys").size(16),
            text_input("OpenAI API key", &state.settings.openai_api_key)
                .on_input(Message::OpenAiKeyChanged)
                .secure(true)
                .padding(8),
            text_input("Grok API key", &state.settings.grok_api_key)
                .on_input(Message::GrokKeyChanged)
                .secure(true)
                .padding(8),
            text_input("GitHub PAT (MCP)", &state.settings.github_pat)
                .on_input(Message::GithubPatChanged)
                .secure(true)
                .padding(8),
            text_input("Zapier token (MCP)", &state.settings.zapier_token)
                .on_input(Message::ZapierTokenChanged)
                .secure(true)
                .padding(8),
        ]
        .spacing(8))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("Runtime policy").size(16),
            text("Memory: enabled (fixed)").size(14),
            text("OpenAI chat model: gpt-4.1-mini (fixed)").size(14),
            text("OpenAI memory models: gpt-4.1-mini / text-embedding-3-small / gpt-4.1-mini (fixed)").size(14),
            text("OpenAI coding model: gpt-5.2-codex (fixed)").size(14),
        ]
        .spacing(6))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("MCP Servers").size(16),
            mcp_rows,
            button("+ Add MCP Server")
                .padding([8, 12])
                .on_press(Message::AddMcpServer),
        ]
        .spacing(8))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("HTTP Call Servers").size(16),
            http_rows,
            button("+ Add HTTP Server")
                .padding([8, 12])
                .on_press(Message::AddHttpServer),
        ]
        .spacing(8))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("Network + system").size(16),
            text("Search default deny: enabled (fixed)").size(14),
            text_input("Search network allow (comma-separated)", &state.settings.search_network_allow)
                .on_input(Message::SearchNetworkAllowChanged)
                .padding(8),
            text_input("Wakeup poll seconds", &state.settings.wakeup_poll_seconds)
                .on_input(Message::WakeupPollSecondsChanged)
                .padding(8),
            text_input("TPM mode (strict|auto|compatible)", &state.settings.tpm_mode)
                .on_input(Message::TpmModeChanged)
                .padding(8),
        ]
        .spacing(8))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("Solana").size(16),
            text("RPC endpoint").size(14),
            text_input("https://...", &state.settings.solana_rpc_endpoint)
                .on_input(Message::SolanaRpcEndpointChanged)
                .padding(8),
            if let Some(address) = &state.solana_wallet_address {
                row![
                    text("Butterfly Bot Wallet address:").size(14),
                    text(address.clone()).size(14),
                    Space::new().width(Length::Fill),
                    button(text("📋").size(14))
                        .padding(6)
                        .width(30)
                        .height(30)
                        .on_press(Message::CopyToClipboard(address.clone())),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
            } else {
                row![
                    text("Butterfly Bot Wallet address: unavailable").size(14),
                    Space::new().width(Length::Fill),
                    button("Refresh")
                        .padding([6, 10])
                        .on_press(Message::RefreshSolanaWallet),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
            },
            if state.solana_wallet_status.is_empty() {
                text("")
            } else {
                text(state.solana_wallet_status.clone()).size(13)
            }
        ]
        .spacing(8))
        .padding(10)
        .style(glass_panel),
        if state.settings_error.is_empty() {
            text(state.settings_status.clone()).color([0.55, 0.9, 0.65])
        } else {
            text(state.settings_error.clone()).color([0.95, 0.45, 0.45])
        }
    ]
    .spacing(12);

    container(
        scrollable(container(form).width(Length::Fill).padding([0, 16]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
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

    let security_lines =
        state
            .security_findings
            .iter()
            .fold(column!().spacing(6), |col, finding| {
                let mut line = format!(
                    "{} [{} / {}] — {}{}",
                    finding.id,
                    finding.severity,
                    finding.status,
                    finding.message,
                    if finding.auto_fixable {
                        " (auto-fixable)"
                    } else {
                        ""
                    }
                );
                if let Some(hint) = &finding.fix_hint {
                    line.push_str(&format!(" ({hint})"));
                }
                col.push(text(line))
            });

    let content = column![
        button("Run security doctor")
            .padding([8, 12])
            .style(iced::widget::button::primary)
            .on_press(Message::RunDoctorPressed),
        text(state.doctor_status.clone()),
        if state.doctor_error.is_empty() {
            text("")
        } else {
            text(state.doctor_error.clone()).color([0.95, 0.45, 0.45])
        },
        text(format!(
            "Doctor overall: {}",
            display_posture_level(&state.doctor_overall)
        )),
        container(doctor_lines).padding(8).style(glass_panel),
        text(""),
        text(state.security_status.clone()),
        if state.security_error.is_empty() {
            text("")
        } else {
            text(state.security_error.clone()).color([0.95, 0.45, 0.45])
        },
        text(format!(
            "Security overall: {}",
            display_posture_level(&state.security_overall)
        )),
        container(security_lines).padding(8).style(glass_panel),
    ]
    .spacing(10);

    container(scrollable(content).height(Length::Fill).width(Length::Fill))
        .padding(10)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_context_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let content = column![
        text("Context prompt").size(22),
        text("Paste markdown text or a URL.").size(14),
        text_editor(&state.context_editor)
            .height(160)
            .padding(10)
            .on_action(Message::ContextEdited),
        row![
            button("Save Context")
                .padding([8, 12])
                .style(iced::widget::button::success)
                .on_press(Message::SaveSettingsPressed),
            button("Reload")
                .padding([8, 12])
                .on_press(Message::LoadSettingsPressed),
        ]
        .spacing(10),
        text("Preview").size(14),
        container(
            scrollable(
                markdown::view(
                    state.context_preview_items.iter(),
                    markdown_render_settings()
                )
                .map(Message::MarkdownLinkClicked)
            )
            .height(Length::Fill)
            .width(Length::Fill)
        )
        .padding(10)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill),
        if state.settings_error.is_empty() {
            text(state.settings_status.clone()).color([0.55, 0.9, 0.65])
        } else {
            text(state.settings_error.clone()).color([0.95, 0.45, 0.45])
        }
    ]
    .spacing(10)
    .width(Length::Fill)
    .height(Length::Fill);

    container(content)
        .padding(10)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_heartbeat_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let content = column![
        text("Heartbeat").size(22),
        text("Paste markdown text or a URL.").size(14),
        text_editor(&state.heartbeat_editor)
            .height(160)
            .padding(10)
            .on_action(Message::HeartbeatEdited),
        row![
            button("Save Heartbeat")
                .padding([8, 12])
                .style(iced::widget::button::success)
                .on_press(Message::SaveSettingsPressed),
            button("Reload")
                .padding([8, 12])
                .on_press(Message::LoadSettingsPressed),
        ]
        .spacing(10),
        text("Preview").size(14),
        container(
            scrollable(
                markdown::view(
                    state.heartbeat_preview_items.iter(),
                    markdown_render_settings()
                )
                .map(Message::MarkdownLinkClicked)
            )
            .height(Length::Fill)
            .width(Length::Fill)
        )
        .padding(10)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill),
        if state.settings_error.is_empty() {
            text(state.settings_status.clone()).color([0.55, 0.9, 0.65])
        } else {
            text(state.settings_error.clone()).color([0.95, 0.45, 0.45])
        }
    ]
    .spacing(10)
    .width(Length::Fill)
    .height(Length::Fill);

    container(content)
        .padding(10)
        .style(glass_panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
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

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(text) = extract_assistant_text(&json) {
            return Ok(text);
        }
    }

    Ok(body)
}

fn extract_assistant_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    for key in ["response", "output", "text", "message", "content", "answer"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
    }

    if let Some(message) = value.get("message") {
        if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
        if let Some(text) = message.get("text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
    }

    if let Some(choice) = value
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
    {
        if let Some(text) = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
        {
            return Some(text.to_string());
        }
        if let Some(text) = choice.get("text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
    }

    None
}

fn daemon_request_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("request client")
}

async fn check_daemon_health(daemon_url: String) -> DaemonHealth {
    let normalized = normalize_daemon_url(&daemon_url);
    if check_daemon_health_once(&normalized).await {
        return DaemonHealth {
            daemon_url: normalized,
            healthy: true,
            switched: false,
        };
    }

    let (host, port) = parse_daemon_address(&normalized);
    if port == 7878 && (host == "127.0.0.1" || host == "localhost") {
        let scheme = if normalized.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        let fallback = format!("{scheme}://{host}:7979");
        if check_daemon_health_once(&fallback).await {
            return DaemonHealth {
                daemon_url: fallback,
                healthy: true,
                switched: true,
            };
        }
    }

    DaemonHealth {
        daemon_url: normalized,
        healthy: false,
        switched: false,
    }
}

async fn check_daemon_health_once(daemon_url: &str) -> bool {
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

async fn fetch_solana_wallet_address(
    daemon_url: String,
    token: String,
    user_id: String,
) -> Result<Option<String>, String> {
    if user_id.trim().is_empty() {
        return Ok(None);
    }

    let client = daemon_request_client();
    let url = format!("{}/solana/wallet", daemon_url.trim_end_matches('/'));
    let mut request = client
        .get(url)
        .query(&[("user_id", user_id.as_str()), ("actor", "agent")]);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }

    let response = request.send().await.map_err(|err| err.to_string())?;
    if response.status().is_success() {
        let parsed = response
            .json::<SolanaWalletUiResponse>()
            .await
            .map_err(|err| err.to_string())?;
        if parsed.address.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(parsed.address))
        }
    } else {
        Ok(None)
    }
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

    let mut parsed_ts: Option<i64> = None;
    if let Some(close_idx) = trimmed.find(']') {
        if trimmed.starts_with('[') && close_idx > 1 {
            let bracket_ts = &trimmed[1..close_idx];
            parsed_ts = parse_history_bracket_timestamp(bracket_ts);
        }
    }

    let normalized = if let Some(idx) = trimmed.find("] ") {
        let (left, right) = trimmed.split_at(idx + 2);
        if left.starts_with('[') {
            right.trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    for (marker, role) in [
        ("user:", MessageRole::User),
        ("assistant:", MessageRole::Bot),
        ("system:", MessageRole::System),
    ] {
        if let Some(value) = normalized.strip_prefix(marker) {
            return Some((role, value.trim().to_string(), parsed_ts));
        }
        let with_space = format!(" {marker}");
        if let Some(pos) = normalized.find(&with_space) {
            let body = normalized[(pos + with_space.len())..].trim();
            return Some((role, body.to_string(), parsed_ts));
        }
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

    Some((role, rest.to_string(), parsed_ts))
}

fn parse_history_bracket_timestamp(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(epoch) = trimmed.parse::<i64>() {
        return Some(epoch);
    }

    const TS_MINUTE: &[::time::format_description::FormatItem<'static>] =
        ::time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]");
    const TS_SECOND: &[::time::format_description::FormatItem<'static>] =
        ::time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

    let parsed = PrimitiveDateTime::parse(trimmed, TS_SECOND)
        .ok()
        .or_else(|| PrimitiveDateTime::parse(trimmed, TS_MINUTE).ok())?;

    Some(parsed.assume_utc().unix_timestamp())
}

fn now_unix_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn format_local_time(ts: i64) -> String {
    let Ok(utc_dt) = OffsetDateTime::from_unix_timestamp(ts) else {
        return "1970-01-01 00:00:00".to_string();
    };

    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_dt = utc_dt.to_offset(local_offset);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local_dt.year(),
        u8::from(local_dt.month()),
        local_dt.day(),
        local_dt.hour(),
        local_dt.minute(),
        local_dt.second()
    )
}

fn display_posture_level(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" | "pass" => "high",
        "warn" | "medium" => "medium",
        "high" | "critical" | "fail" => "low",
        _ => "unknown",
    }
}

fn derive_doctor_overall(checks: &[DoctorCheckResponse]) -> &'static str {
    let has_fail = checks.iter().any(|check| check.status == "fail");
    let has_warn = checks.iter().any(|check| check.status == "warn");

    if has_fail {
        "fail"
    } else if has_warn {
        "warn"
    } else {
        "pass"
    }
}

fn parse_markdown_items(input: &str) -> Vec<markdown::Item> {
    markdown::parse(input).collect()
}

fn markdown_render_settings() -> markdown::Settings {
    let mut settings = markdown::Settings::with_text_size(15, Theme::Dark);
    settings.h1_size = 30.0.into();
    settings.h2_size = 25.0.into();
    settings.h3_size = 21.0.into();
    settings.h4_size = 18.0.into();
    settings.h5_size = 16.0.into();
    settings.h6_size = 15.0.into();
    settings.code_size = 13.0.into();
    settings.spacing = 10.0.into();
    settings
}

fn open_uri_best_effort(uri: &str) -> std::io::Result<()> {
    if uri.trim().is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(uri)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(uri)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", uri])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

async fn check_external_start_status(daemon_url: String) -> Result<String, String> {
    let health = check_daemon_health(daemon_url).await;
    if health.healthy {
        if health.switched {
            Ok(format!("External daemon healthy ({})", health.daemon_url))
        } else {
            Ok("External daemon healthy".to_string())
        }
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

async fn start_local_daemon(
    daemon_url: String,
    db_path: String,
    token: String,
) -> Result<String, String> {
    let (host, port) = parse_daemon_address(&daemon_url);
    let mut selected = None;
    for candidate in daemon_binary_candidates() {
        if candidate.exists() || candidate == Path::new("butterfly-botd") {
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

async fn stop_daemon_by_url(daemon_url: String) -> Result<String, String> {
    match stop_local_daemon().await {
        Ok(status) if status == "Daemon stopped" => Ok(status),
        Ok(_) | Err(_) => {
            let (_host, port) = parse_daemon_address(&daemon_url);

            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let pattern = format!("butterfly-botd.*--port {port}");
                let status = Command::new("pkill")
                    .arg("-f")
                    .arg(&pattern)
                    .status()
                    .map_err(|err| format!("Failed to stop daemon process: {err}"))?;

                if !status.success() {
                    let _ = Command::new("pkill")
                        .arg("-f")
                        .arg("butterfly-botd")
                        .status();
                }

                tokio::time::sleep(Duration::from_millis(150)).await;
                let _ = Command::new("pkill")
                    .arg("-9")
                    .arg("-f")
                    .arg(&pattern)
                    .status();
                let _ = Command::new("pkill")
                    .arg("-9")
                    .arg("-f")
                    .arg("butterfly-botd")
                    .status();

                tokio::time::sleep(Duration::from_millis(300)).await;
                if !check_daemon_health_once(&daemon_url).await {
                    return Ok("Daemon stopped".to_string());
                }

                Err("Daemon still reachable after stop attempt".to_string())
            }

            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                Err("No local daemon process to stop".to_string())
            }
        }
    }
}

fn stop_local_daemon_blocking() -> Result<String, String> {
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

fn kill_all_daemons_best_effort() {
    let _ = stop_local_daemon_blocking();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let _ = Command::new("pkill")
            .arg("-f")
            .arg("butterfly-botd")
            .status();
        std::thread::sleep(std::time::Duration::from_millis(120));
        let _ = Command::new("pkill")
            .arg("-9")
            .arg("-f")
            .arg("butterfly-botd")
            .status();
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
        fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
            let mut cursor = value;
            for key in path {
                cursor = cursor.get(*key)?;
            }
            Some(cursor)
        }

        fn parse_server_rows(value: Option<&Value>) -> Vec<UiServerRow> {
            value
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
                            if name.is_empty() && url.is_empty() {
                                return None;
                            }
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
                            Some(UiServerRow {
                                name,
                                url,
                                header_key,
                                header_value,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }

        let mut config = crate::config::Config::from_store(&db_path)
            .map_err(|err| format!("Failed to load config: {err}"))?;

        if config.provider.is_none() {
            config.provider = Some(crate::config::ProviderConfig {
                runtime: crate::config::RuntimeProvider::Openai,
            });
        }

        let github_pat = crate::vault::get_secret_required("github_pat")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let zapier_token = crate::vault::get_secret_required("zapier_token")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let openai_api_key = crate::vault::get_secret_required("openai_api_key")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();
        let grok_api_key = crate::vault::get_secret_required("search_internet_grok_api_key")
            .map_err(|err| err.to_string())?
            .unwrap_or_default();

        let tools = config.tools.as_ref().unwrap_or(&Value::Null);
        let solana_rpc_endpoint = get_path(tools, &["settings", "solana", "rpc", "endpoint"])
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let search_network_allow = get_path(tools, &["settings", "permissions", "network_allow"])
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let wakeup_poll_seconds = get_path(tools, &["wakeup", "poll_seconds"])
            .and_then(|v| v.as_u64())
            .unwrap_or(60)
            .to_string();
        let tpm_mode = get_path(tools, &["settings", "security", "tpm_mode"])
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();
        let mut mcp_servers = parse_server_rows(get_path(tools, &["mcp", "servers"]));
        let mut http_call_servers = parse_server_rows(get_path(tools, &["http_call", "servers"]));

        if http_call_servers.is_empty() {
            let shared_header = get_path(tools, &["http_call", "custom_headers"])
                .and_then(|v| v.as_object())
                .and_then(|map| {
                    map.iter().find_map(|(k, v)| {
                        v.as_str()
                            .map(|value| (k.trim().to_string(), value.trim().to_string()))
                    })
                })
                .unwrap_or_else(|| (String::new(), String::new()));

            if let Some(base_urls) =
                get_path(tools, &["http_call", "base_urls"]).and_then(|v| v.as_array())
            {
                for (index, value) in base_urls.iter().enumerate() {
                    if let Some(url) = value.as_str() {
                        let trimmed = url.trim();
                        if !trimmed.is_empty() {
                            http_call_servers.push(UiServerRow {
                                name: format!("server_{}", index + 1),
                                url: trimmed.to_string(),
                                header_key: shared_header.0.clone(),
                                header_value: shared_header.1.clone(),
                            });
                        }
                    }
                }
            }
        }

        let prompt_text = match &config.prompt_source {
            crate::config::MarkdownSource::Url { url } => url.clone(),
            crate::config::MarkdownSource::Database { markdown } => markdown.clone(),
        };
        let heartbeat_text = match &config.heartbeat_source {
            crate::config::MarkdownSource::Url { url } => url.clone(),
            crate::config::MarkdownSource::Database { markdown } => markdown.clone(),
        };

        Ok(LoadedSettings {
            form: SettingsForm {
                github_pat,
                zapier_token,
                openai_api_key,
                grok_api_key,
                solana_rpc_endpoint,
                search_network_allow,
                wakeup_poll_seconds,
                tpm_mode,
                mcp_servers: std::mem::take(&mut mcp_servers),
                http_call_servers: std::mem::take(&mut http_call_servers),
                prompt_text,
                heartbeat_text,
            },
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

            config.provider = Some(crate::config::ProviderConfig {
                runtime: crate::config::RuntimeProvider::Openai,
            });

            let openai = config.openai.get_or_insert(crate::config::OpenAiConfig {
                api_key: None,
                model: None,
                base_url: None,
            });
            openai.base_url = Some("https://api.openai.com/v1".to_string());
            openai.model = Some("gpt-4.1-mini".to_string());
            openai.api_key = None;

            let memory = config.memory.get_or_insert(crate::config::MemoryConfig {
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
            memory.enabled = Some(true);
            memory.summary_model = Some("gpt-4.1-mini".to_string());
            memory.embedding_model = Some("text-embedding-3-small".to_string());
            memory.rerank_model = Some("gpt-4.1-mini".to_string());
            memory.openai = None;

            config.prompt_source = markdown_source_from_input(&form.prompt_text);
            config.heartbeat_source = markdown_source_from_input(&form.heartbeat_text);

            crate::vault::set_secret_required("github_pat", &form.github_pat)
                .map_err(|err| format!("Failed to store GitHub PAT: {err}"))?;
            crate::vault::set_secret_required("zapier_token", &form.zapier_token)
                .map_err(|err| format!("Failed to store Zapier token: {err}"))?;
            crate::vault::set_secret_required("openai_api_key", &form.openai_api_key)
                .map_err(|err| format!("Failed to store OpenAI API key: {err}"))?;
            crate::vault::set_secret_required("search_internet_grok_api_key", &form.grok_api_key)
                .map_err(|err| format!("Failed to store search API key: {err}"))?;

            let tools = config
                .tools
                .get_or_insert_with(|| Value::Object(serde_json::Map::new()));
            let tools_obj = tools
                .as_object_mut()
                .ok_or_else(|| "tools must be an object".to_string())?;
            {
                let settings = tools_obj
                    .entry("settings")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let settings_obj = settings
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings must be an object".to_string())?;
                let permissions = settings_obj
                    .entry("permissions")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let permissions_obj = permissions
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings.permissions must be an object".to_string())?;
                permissions_obj.insert("default_deny".to_string(), Value::Bool(true));
                let allow_items = form
                    .search_network_allow
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect::<Vec<_>>();
                permissions_obj.insert("network_allow".to_string(), Value::Array(allow_items));

                let security = settings_obj
                    .entry("security")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let security_obj = security
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings.security must be an object".to_string())?;
                security_obj.insert("tpm_mode".to_string(), Value::String(form.tpm_mode.clone()));

                let solana = settings_obj
                    .entry("solana")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let solana_obj = solana
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings.solana must be an object".to_string())?;
                let rpc = solana_obj
                    .entry("rpc")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let rpc_obj = rpc
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings.solana.rpc must be an object".to_string())?;
                rpc_obj.insert(
                    "endpoint".to_string(),
                    Value::String(form.solana_rpc_endpoint.clone()),
                );
            }
            std::env::set_var("BUTTERFLY_TPM_MODE", &form.tpm_mode);

            let wakeup = tools_obj
                .entry("wakeup")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let wakeup_obj = wakeup
                .as_object_mut()
                .ok_or_else(|| "tools.wakeup must be an object".to_string())?;
            let poll_seconds = form.wakeup_poll_seconds.trim().parse::<u64>().unwrap_or(60);
            wakeup_obj.insert(
                "poll_seconds".to_string(),
                Value::Number(serde_json::Number::from(poll_seconds)),
            );

            let search_internet = tools_obj
                .entry("search_internet")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let search_internet_obj = search_internet
                .as_object_mut()
                .ok_or_else(|| "tools.search_internet must be an object".to_string())?;
            let search_settings = search_internet_obj
                .entry("settings")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let search_settings_obj = search_settings
                .as_object_mut()
                .ok_or_else(|| "tools.search_internet.settings must be an object".to_string())?;
            search_settings_obj.insert("provider".to_string(), Value::String("grok".to_string()));

            let mcp = tools_obj
                .entry("mcp")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let mcp_obj = mcp
                .as_object_mut()
                .ok_or_else(|| "tools.mcp must be an object".to_string())?;
            let mcp_servers = form
                .mcp_servers
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    if name.is_empty() || url.is_empty() {
                        return None;
                    }
                    let mut server = serde_json::Map::new();
                    server.insert("name".to_string(), Value::String(name.to_string()));
                    server.insert("url".to_string(), Value::String(url.to_string()));
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    if !header_key.is_empty() && !header_value.is_empty() {
                        let mut headers = serde_json::Map::new();
                        headers.insert(
                            header_key.to_string(),
                            Value::String(header_value.to_string()),
                        );
                        server.insert("headers".to_string(), Value::Object(headers));
                    }
                    Some(Value::Object(server))
                })
                .collect::<Vec<_>>();
            mcp_obj.insert("servers".to_string(), Value::Array(mcp_servers));

            let coding = tools_obj
                .entry("coding")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let coding_obj = coding
                .as_object_mut()
                .ok_or_else(|| "tools.coding must be an object".to_string())?;
            coding_obj.insert(
                "model".to_string(),
                Value::String("gpt-5.2-codex".to_string()),
            );
            coding_obj.insert(
                "base_url".to_string(),
                Value::String("https://api.openai.com/v1".to_string()),
            );

            let http_call = tools_obj
                .entry("http_call")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let http_call_obj = http_call
                .as_object_mut()
                .ok_or_else(|| "tools.http_call must be an object".to_string())?;
            let http_servers = form
                .http_call_servers
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.trim();
                    let url = entry.url.trim();
                    if name.is_empty() || url.is_empty() {
                        return None;
                    }
                    let mut server = serde_json::Map::new();
                    server.insert("name".to_string(), Value::String(name.to_string()));
                    server.insert("url".to_string(), Value::String(url.to_string()));
                    let header_key = entry.header_key.trim();
                    let header_value = entry.header_value.trim();
                    if !header_key.is_empty() && !header_value.is_empty() {
                        let mut headers = serde_json::Map::new();
                        headers.insert(
                            header_key.to_string(),
                            Value::String(header_value.to_string()),
                        );
                        server.insert("headers".to_string(), Value::Object(headers));
                    }
                    Some(Value::Object(server))
                })
                .collect::<Vec<_>>();
            http_call_obj.insert("servers".to_string(), Value::Array(http_servers.clone()));

            let base_urls = http_servers
                .iter()
                .filter_map(|value| {
                    value
                        .get("url")
                        .and_then(|url| url.as_str())
                        .map(|url| Value::String(url.to_string()))
                })
                .collect::<Vec<_>>();
            http_call_obj.insert("base_urls".to_string(), Value::Array(base_urls));

            crate::config_store::save_config(&db_path, &config)
                .map_err(|err| format!("Failed to save config: {err}"))?;

            Ok::<crate::config::Config, String>(config)
        }
    })
    .await
    .map_err(|err| err.to_string())??;

    let pretty = serde_json::to_string_pretty(&config).map_err(|err| err.to_string())?;
    let _ = tokio::task::spawn_blocking(move || {
        crate::vault::set_secret_required("app_config_json", &pretty)
    })
    .await;

    let client = daemon_request_client();
    let url = format!("{}/reload_config", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let _ = request.send().await;

    Ok("Settings saved".to_string())
}
