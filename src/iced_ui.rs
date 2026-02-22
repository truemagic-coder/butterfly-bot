use ::time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use chrono::{DateTime, Local, TimeZone};
use chrono_english::{parse_date_string, Dialect};
use iced::widget::{
    button, column, container, image, markdown, row, scrollable, text, text_editor, text_input,
    Id as WidgetId, Space,
};
use iced::{
    application, time, Background, Border, Color, Element, Length, Shadow, Size, Subscription,
    Task, Theme,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::inbox_fsm::InboxState as InboxStatus;

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

#[derive(Clone, Debug, Deserialize)]
struct InboxApiResponse {
    items: Vec<InboxApiItem>,
}

#[derive(Clone, Debug, Deserialize)]
struct InboxApiItem {
    id: String,
    source_type: String,
    owner: String,
    title: String,
    details: Option<String>,
    status: String,
    priority: String,
    due_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    requires_human_action: bool,
    origin_ref: String,
    #[serde(default)]
    dependency_refs: Vec<String>,
    t_shirt_size: Option<String>,
    story_points: Option<i32>,
    estimate_optimistic_minutes: Option<i32>,
    estimate_likely_minutes: Option<i32>,
    estimate_pessimistic_minutes: Option<i32>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuditEventsApiResponse {
    events: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReminderDeliveryEventsApiResponse {
    events: Vec<Value>,
}

#[derive(Clone, Debug)]
struct AuditEventRow {
    timestamp: i64,
    event_type: String,
    status: String,
    actor: Option<String>,
    line: String,
    origin_ref: Option<String>,
}

#[derive(Clone)]
struct ChatMessage {
    id: u64,
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
    Inbox,
    Kanban,
    Dependencies,
    Gantt,
    Audit,
    Chat,
    Activity,
    Settings,
    Diagnostics,
    Context,
    Heartbeat,
}

impl UiTab {
    fn all() -> [UiTab; 11] {
        [
            UiTab::Inbox,
            UiTab::Kanban,
            UiTab::Dependencies,
            UiTab::Gantt,
            UiTab::Audit,
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
            UiTab::Inbox => "Inbox",
            UiTab::Kanban => "Kanban",
            UiTab::Dependencies => "Dependencies",
            UiTab::Gantt => "Gantt",
            UiTab::Audit => "Audit",
            UiTab::Chat => "Chat",
            UiTab::Activity => "Activity",
            UiTab::Settings => "Config",
            UiTab::Diagnostics => "Diagnostics",
            UiTab::Context => "Context",
            UiTab::Heartbeat => "Heartbeat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboxSourceType {
    Reminder,
    Todo,
    Task,
    PlanStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboxPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Clone, Debug)]
struct InboxItem {
    id: String,
    source_type: InboxSourceType,
    owner: String,
    title: String,
    details: Option<String>,
    status: InboxStatus,
    priority: InboxPriority,
    due_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    requires_human_action: bool,
    origin_ref: String,
    dependency_refs: Vec<String>,
    t_shirt_size: Option<String>,
    story_points: Option<i32>,
    estimate_optimistic_minutes: Option<i32>,
    estimate_likely_minutes: Option<i32>,
    estimate_pessimistic_minutes: Option<i32>,
}

#[derive(Clone, Copy, Debug)]
enum InboxActionKind {
    Acknowledge,
    Start,
    Block,
    Done,
    Reopen,
    Snooze,
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
    proactive_chat_enabled: bool,
    proactive_chat_min_interval_seconds: String,
    proactive_chat_severity: String,
    proactive_chat_quiet_start_hhmm: String,
    proactive_chat_quiet_end_hhmm: String,
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
            proactive_chat_enabled: true,
            proactive_chat_min_interval_seconds: "45".to_string(),
            proactive_chat_severity: "blocked_or_overdue".to_string(),
            proactive_chat_quiet_start_hhmm: String::new(),
            proactive_chat_quiet_end_hhmm: String::new(),
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
    inbox_items: Vec<InboxItem>,
    inbox_status: String,
    inbox_error: String,
    inbox_refresh_in_flight: bool,
    inbox_action_origin_ref_in_flight: Option<String>,
    inbox_last_refresh_ts: i64,
    last_badge_actionable_count: Option<usize>,
    audit_events: Vec<AuditEventRow>,
    audit_status: String,
    audit_error: String,
    audit_refresh_in_flight: bool,
    audit_last_refresh_ts: i64,
    audit_last_activity_bridge_ts: i64,
    audit_origin_filter: Option<String>,
    timeline_focus_origin_ref: Option<String>,
    chat_origin_anchor: Option<String>,
    chat_anchor_message_id: Option<u64>,
    chat_scroll_id: WidgetId,
    proactive_notified_origin_refs: HashSet<String>,
    proactive_last_chat_ts: i64,
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
    reminder_delivery_status: String,
    reminder_delivery_error: String,
    reminder_delivery_events: Vec<String>,
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
    ToggleProactiveChatEnabled,
    ProactiveChatMinIntervalChanged(String),
    ProactiveChatSeverityChanged(String),
    ProactiveChatQuietStartChanged(String),
    ProactiveChatQuietEndChanged(String),
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
    InboxRefreshRequested,
    InboxLoaded(Result<Vec<InboxItem>, String>),
    InboxAcknowledge(String),
    InboxStart(String),
    InboxBlock(String),
    InboxDone(String),
    InboxReopen(String),
    InboxSnooze(String),
    InboxActionFinished(Result<String, String>),
    RefreshReminderDeliveryEvents,
    ReminderDeliveryEventsLoaded(Result<Vec<String>, String>),
    AuditRefreshRequested,
    AuditEventsLoaded(Result<Vec<AuditEventRow>, String>),
    TimelineOpenItem(String),
    TimelineOpenAudit(String),
    OpenChatWithContext(String),
    OpenChatAtEvent(String, i64),
    ChatClearAnchor,
    AuditClearFilter,
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
                Task::perform(
                    load_inbox_items(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                    ),
                    Message::InboxLoaded,
                ),
                Task::perform(
                    fetch_audit_events(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                        200,
                    ),
                    Message::AuditEventsLoaded,
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

fn build_stamp() -> String {
    format!(
        "v{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("BUTTERFLY_GIT_SHA").unwrap_or("dev")
    )
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
            inbox_items: vec![],
            inbox_status: "Loading inbox...".to_string(),
            inbox_error: String::new(),
            inbox_refresh_in_flight: true,
            inbox_action_origin_ref_in_flight: None,
            inbox_last_refresh_ts: 0,
            last_badge_actionable_count: None,
            audit_events: vec![],
            audit_status: "Loading audit events...".to_string(),
            audit_error: String::new(),
            audit_refresh_in_flight: true,
            audit_last_refresh_ts: 0,
            audit_last_activity_bridge_ts: 0,
            audit_origin_filter: None,
            timeline_focus_origin_ref: None,
            chat_origin_anchor: None,
            chat_anchor_message_id: None,
            chat_scroll_id: WidgetId::unique(),
            proactive_notified_origin_refs: HashSet::new(),
            proactive_last_chat_ts: 0,
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
            reminder_delivery_status: String::new(),
            reminder_delivery_error: String::new(),
            reminder_delivery_events: vec![],
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
            id: self.next_id,
            role,
            text,
            markdown_items,
            timestamp: now_unix_ts(),
        });
        self.next_id = self.next_id.saturating_add(1);
        if self.chat_messages.len() > 300 {
            let drop_count = self.chat_messages.len() - 300;
            self.chat_messages.drain(0..drop_count);
        }
    }

    fn push_activity(&mut self, text: String) {
        let markdown_items = parse_markdown_items(&text);
        self.activity_messages.push(ChatMessage {
            id: self.next_id,
            role: MessageRole::System,
            text,
            markdown_items,
            timestamp: now_unix_ts(),
        });
        self.next_id = self.next_id.saturating_add(1);
        if self.activity_messages.len() > 300 {
            let drop_count = self.activity_messages.len() - 300;
            self.activity_messages.drain(0..drop_count);
        }
    }
}

impl Drop for ButterflyIcedApp {
    fn drop(&mut self) {
        let _ = set_macos_dock_badge(0);
        if self.manage_local_daemon {
            let _ = stop_local_daemon_blocking();
        }
    }
}

fn update(state: &mut ButterflyIcedApp, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            let mut tasks: Vec<Task<Message>> = Vec::new();
            if !state.daemon_starting {
                tasks.push(Task::perform(
                    check_daemon_health(state.daemon_url.clone()),
                    Message::HealthChecked,
                ));
            }

            let now = now_unix_ts();
            if !state.inbox_refresh_in_flight
                && now.saturating_sub(state.inbox_last_refresh_ts) >= 15
            {
                state.inbox_refresh_in_flight = true;
                tasks.push(Task::perform(
                    load_inbox_items(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                    ),
                    Message::InboxLoaded,
                ));
            }

            if !state.audit_refresh_in_flight
                && now.saturating_sub(state.audit_last_refresh_ts) >= 15
            {
                state.audit_refresh_in_flight = true;
                tasks.push(Task::perform(
                    fetch_audit_events(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                        200,
                    ),
                    Message::AuditEventsLoaded,
                ));
            }

            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        Message::TabSelected(tab) => {
            state.active_tab = tab;
            if tab == UiTab::Inbox && !state.inbox_refresh_in_flight {
                state.inbox_refresh_in_flight = true;
                return Task::perform(
                    load_inbox_items(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                    ),
                    Message::InboxLoaded,
                );
            }
            if tab == UiTab::Audit && !state.audit_refresh_in_flight {
                state.audit_refresh_in_flight = true;
                return Task::perform(
                    fetch_audit_events(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                        200,
                    ),
                    Message::AuditEventsLoaded,
                );
            }
            Task::none()
        }
        Message::TimelineOpenItem(origin_ref) => {
            state.timeline_focus_origin_ref = Some(origin_ref.clone());
            state.active_tab = UiTab::Inbox;
            state.push_activity(format!("timeline drilldown → inbox ({origin_ref})"));
            if !state.inbox_refresh_in_flight {
                state.inbox_refresh_in_flight = true;
                return Task::perform(
                    load_inbox_items(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                    ),
                    Message::InboxLoaded,
                );
            }
            Task::none()
        }
        Message::TimelineOpenAudit(origin_ref) => {
            state.audit_origin_filter = Some(origin_ref.clone());
            state.active_tab = UiTab::Audit;
            state.push_activity(format!("timeline drilldown → audit ({origin_ref})"));
            if !state.audit_refresh_in_flight {
                state.audit_refresh_in_flight = true;
                return Task::perform(
                    fetch_audit_events(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                        200,
                    ),
                    Message::AuditEventsLoaded,
                );
            }
            Task::none()
        }
        Message::OpenChatWithContext(origin_ref) => {
            state.active_tab = UiTab::Chat;
            state.chat_origin_anchor = Some(origin_ref.clone());
            state.chat_anchor_message_id =
                find_latest_chat_message_id(&state.chat_messages, &origin_ref, None);
            state.composer = format!(
                "Show full context and latest state for work item {origin_ref}. Include blockers and next action."
            );
            state.push_activity(format!("drilldown → chat context ({origin_ref})"));
            scroll_chat_to_anchor_task(state)
        }
        Message::OpenChatAtEvent(origin_ref, event_ts) => {
            state.active_tab = UiTab::Chat;
            state.chat_origin_anchor = Some(origin_ref.clone());
            state.chat_anchor_message_id =
                find_latest_chat_message_id(&state.chat_messages, &origin_ref, Some(event_ts));
            state.composer = format!(
                "Show full context around event time {} for work item {origin_ref}.",
                format_local_time(event_ts)
            );
            state.push_activity(format!(
                "drilldown → chat anchor ({origin_ref}) @ {}",
                format_local_time(event_ts)
            ));
            scroll_chat_to_anchor_task(state)
        }
        Message::ChatClearAnchor => {
            state.chat_origin_anchor = None;
            state.chat_anchor_message_id = None;
            Task::none()
        }
        Message::AuditClearFilter => {
            state.audit_origin_filter = None;
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
                    state.activity_messages.clear();
                    state.next_id = 1;
                    for line in lines {
                        if let Some((role, text, ts)) = parse_history_entry(&line) {
                            if role == MessageRole::System {
                                state.push_activity(format!("history/system • {}", text));
                                continue;
                            }
                            let markdown_items = parse_markdown_items(&text);
                            state.chat_messages.push(ChatMessage {
                                id: state.next_id,
                                role,
                                text,
                                markdown_items,
                                timestamp: ts.unwrap_or_else(now_unix_ts),
                            });
                            state.next_id = state.next_id.saturating_add(1);
                        }
                    }
                    state.chat_anchor_message_id =
                        state.chat_origin_anchor.as_ref().and_then(|origin| {
                            find_latest_chat_message_id(&state.chat_messages, origin, None)
                        });
                    state.push_activity("chat history loaded".to_string());
                    return scroll_chat_to_anchor_task(state);
                }
                Err(err) => {
                    state.error = format!("History load failed: {err}");
                }
            }
            Task::none()
        }
        Message::ClearHistoryPressed => Task::perform(
            run_clear_user_data_request(
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
                    state.activity_messages.clear();
                    state.inbox_items.clear();
                    state.audit_events.clear();
                    state.push_activity("all user work data cleared".to_string());
                    state.inbox_refresh_in_flight = true;
                    return Task::batch(vec![
                        Task::perform(
                            load_inbox_items(
                                state.daemon_url.clone(),
                                state.token.clone(),
                                state.user_id.clone(),
                            ),
                            Message::InboxLoaded,
                        ),
                        Task::perform(
                            fetch_audit_events(
                                state.daemon_url.clone(),
                                state.token.clone(),
                                state.user_id.clone(),
                                200,
                            ),
                            Message::AuditEventsLoaded,
                        ),
                    ]);
                }
                Err(err) => state.error = format!("Clear data failed: {err}"),
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
        Message::ToggleProactiveChatEnabled => {
            state.settings.proactive_chat_enabled = !state.settings.proactive_chat_enabled;
            Task::none()
        }
        Message::ProactiveChatMinIntervalChanged(value) => {
            state.settings.proactive_chat_min_interval_seconds = value;
            Task::none()
        }
        Message::ProactiveChatSeverityChanged(value) => {
            state.settings.proactive_chat_severity = value;
            Task::none()
        }
        Message::ProactiveChatQuietStartChanged(value) => {
            state.settings.proactive_chat_quiet_start_hhmm = value;
            Task::none()
        }
        Message::ProactiveChatQuietEndChanged(value) => {
            state.settings.proactive_chat_quiet_end_hhmm = value;
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
                state.reminder_delivery_error = "Daemon is not running".to_string();
                return Task::none();
            }
            state.doctor_status = "Running security doctor...".to_string();
            state.security_status = "Running security audit...".to_string();
            state.reminder_delivery_status = "Loading reminder delivery diagnostics...".to_string();
            state.doctor_error.clear();
            state.security_error.clear();
            state.reminder_delivery_error.clear();
            Task::batch(vec![
                Task::perform(
                    run_doctor_request(state.daemon_url.clone(), state.token.clone()),
                    Message::DoctorFinished,
                ),
                Task::perform(
                    run_security_audit_request(state.daemon_url.clone(), state.token.clone()),
                    Message::SecurityFinished,
                ),
                Task::perform(
                    fetch_reminder_delivery_events(
                        state.daemon_url.clone(),
                        state.token.clone(),
                        state.user_id.clone(),
                        60,
                    ),
                    Message::ReminderDeliveryEventsLoaded,
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
        Message::InboxRefreshRequested => {
            if state.inbox_refresh_in_flight {
                return Task::none();
            }
            state.inbox_refresh_in_flight = true;
            state.inbox_error.clear();
            Task::perform(
                load_inbox_items(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                ),
                Message::InboxLoaded,
            )
        }
        Message::InboxLoaded(result) => {
            state.inbox_refresh_in_flight = false;
            state.inbox_action_origin_ref_in_flight = None;
            state.inbox_last_refresh_ts = now_unix_ts();
            match result {
                Ok(items) => {
                    state.inbox_items = items;
                    state.inbox_error.clear();
                    state.inbox_status =
                        format!("Inbox synced ({} items)", state.inbox_items.len());
                    sync_actionable_badge(state);
                    maybe_emit_proactive_chat_nudge(state);
                }
                Err(err) => {
                    state.inbox_error = err;
                    state.inbox_status.clear();
                }
            }
            Task::none()
        }
        Message::InboxAcknowledge(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Acknowledge);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Acknowledge,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxStart(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Start);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Start,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxBlock(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Block);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Block,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxDone(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Done);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Done,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxReopen(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Reopen);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Reopen,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxSnooze(origin_ref) => {
            let Some(item) = state
                .inbox_items
                .iter()
                .find(|item| item.origin_ref == origin_ref)
                .cloned()
            else {
                return Task::none();
            };

            optimistic_inbox_transition(state, &origin_ref, InboxActionKind::Snooze);
            state.inbox_action_origin_ref_in_flight = Some(origin_ref.clone());
            state.inbox_refresh_in_flight = true;
            Task::perform(
                apply_inbox_action(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    item,
                    InboxActionKind::Snooze,
                ),
                Message::InboxActionFinished,
            )
        }
        Message::InboxActionFinished(result) => {
            let mut tasks = Vec::new();
            state.inbox_action_origin_ref_in_flight = None;
            match result {
                Ok(status) => {
                    state.push_activity(status.clone());
                    state.inbox_status = status;
                    state.inbox_error.clear();
                }
                Err(err) => {
                    state.inbox_error = err;
                    state.inbox_refresh_in_flight = false;
                    return Task::none();
                }
            }

            tasks.push(Task::perform(
                load_inbox_items(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                ),
                Message::InboxLoaded,
            ));

            Task::batch(tasks)
        }
        Message::RefreshReminderDeliveryEvents => {
            if !state.daemon_running {
                state.reminder_delivery_error = "Daemon is not running".to_string();
                return Task::none();
            }
            state.reminder_delivery_status = "Loading reminder delivery diagnostics...".to_string();
            state.reminder_delivery_error.clear();
            Task::perform(
                fetch_reminder_delivery_events(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    60,
                ),
                Message::ReminderDeliveryEventsLoaded,
            )
        }
        Message::ReminderDeliveryEventsLoaded(result) => {
            match result {
                Ok(events) => {
                    state.reminder_delivery_events = events;
                    state.reminder_delivery_error.clear();
                    state.reminder_delivery_status = format!(
                        "Reminder delivery diagnostics loaded ({})",
                        state.reminder_delivery_events.len()
                    );
                }
                Err(err) => {
                    state.reminder_delivery_error = err;
                    state.reminder_delivery_status.clear();
                }
            }
            Task::none()
        }
        Message::AuditRefreshRequested => {
            if state.audit_refresh_in_flight {
                return Task::none();
            }
            state.audit_refresh_in_flight = true;
            state.audit_error.clear();
            Task::perform(
                fetch_audit_events(
                    state.daemon_url.clone(),
                    state.token.clone(),
                    state.user_id.clone(),
                    200,
                ),
                Message::AuditEventsLoaded,
            )
        }
        Message::AuditEventsLoaded(result) => {
            state.audit_refresh_in_flight = false;
            state.audit_last_refresh_ts = now_unix_ts();
            match result {
                Ok(events) => {
                    let mut bridged = 0usize;
                    let bridge_after_ts = state.audit_last_activity_bridge_ts;
                    for event in events
                        .iter()
                        .filter(|event| event.timestamp > bridge_after_ts)
                    {
                        let should_bridge = matches!(
                            event.event_type.as_str(),
                            "inbox_transition" | "reminder_delivery"
                        );
                        if should_bridge {
                            let origin = event.origin_ref.as_deref().unwrap_or("-");
                            let actor = event.actor.as_deref().unwrap_or("system");
                            state.push_activity(format!(
                                "Ops update ({actor}) • {} • {} • origin:{}",
                                event.event_type, event.status, origin
                            ));
                            bridged += 1;
                        }
                    }
                    if let Some(max_ts) = events.iter().map(|e| e.timestamp).max() {
                        state.audit_last_activity_bridge_ts =
                            state.audit_last_activity_bridge_ts.max(max_ts);
                    }
                    state.audit_events = events;
                    state.audit_error.clear();
                    state.audit_status = format!("Audit synced ({})", state.audit_events.len());
                    if bridged > 0 {
                        state.push_activity(format!("audit→activity bridge: {bridged} updates"));
                    }
                }
                Err(err) => {
                    state.audit_error = err;
                    state.audit_status.clear();
                }
            }
            Task::none()
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
                        rounded_primary_button
                    } else {
                        rounded_secondary_button
                    })
                    .on_press(Message::TabSelected(tab)),
            )
        });

    let daemon_controls = row![
        button(if state.daemon_starting { "⏳" } else { "▶" })
            .padding([8, 12])
            .style(rounded_primary_button)
            .on_press_maybe(
                (!state.daemon_starting && !state.daemon_running)
                    .then_some(Message::StartDaemonPressed)
            ),
        button("⏹")
            .padding([8, 12])
            .style(rounded_primary_button)
            .on_press_maybe(
                (!state.daemon_starting && state.daemon_running)
                    .then_some(Message::StopDaemonPressed)
            ),
        button("🗑")
            .padding([8, 12])
            .style(rounded_danger_button)
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
        UiTab::Inbox => view_inbox_tab(state),
        UiTab::Kanban => view_kanban_tab(state),
        UiTab::Dependencies => view_dependencies_tab(state),
        UiTab::Gantt => view_gantt_tab(state),
        UiTab::Audit => view_audit_tab(state),
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

    let actionable_count = count_actionable_human_items(&state.inbox_items);

    let content = column![
        row![
            logo,
            column![
                text("Butterfly Bot").size(30),
                text("Personal-ops assistant").size(14),
                text(build_stamp()).size(12)
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            text(format!("Actionable now: {actionable_count}")).size(14),
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

fn glass_alert_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::from_rgb(1.0, 0.92, 0.92)),
        background: Some(Background::Color(Color::from_rgba(0.42, 0.12, 0.18, 0.58))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(1.0, 0.35, 0.40, 0.55),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_accent_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(0.20, 0.27, 0.62, 0.56))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.58, 0.70, 1.0, 0.50),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_success_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(0.10, 0.46, 0.30, 0.58))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.45, 0.90, 0.65, 0.52),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_warning_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::WHITE),
        background: Some(Background::Color(Color::from_rgba(0.52, 0.36, 0.10, 0.58))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.98, 0.79, 0.42, 0.50),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn glass_muted_panel(_theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(Color::from_rgb(0.75, 0.79, 0.88)),
        background: Some(Background::Color(Color::from_rgba(0.14, 0.16, 0.22, 0.50))),
        border: Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.62, 0.68, 0.82, 0.28),
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

fn rounded_button_style(base: iced::widget::button::Style) -> iced::widget::button::Style {
    let mut style = base;
    style.border.radius = 999.0.into();
    style
}

fn rounded_secondary_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    rounded_button_style(iced::widget::button::secondary(theme, status))
}

fn rounded_primary_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    rounded_button_style(iced::widget::button::primary(theme, status))
}

fn rounded_success_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    rounded_button_style(iced::widget::button::success(theme, status))
}

fn rounded_danger_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    rounded_button_style(iced::widget::button::danger(theme, status))
}

fn view_inbox_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let now = now_unix_ts();
    let actionable_now = state
        .inbox_items
        .iter()
        .filter(|item| item.requires_human_action && item.status.is_actionable())
        .count();
    let overdue = state
        .inbox_items
        .iter()
        .filter(|item| {
            item.requires_human_action
                && item.status.is_actionable()
                && item.due_at.map(|due| due < now).unwrap_or(false)
        })
        .count();
    let blocked = state
        .inbox_items
        .iter()
        .filter(|item| item.status == InboxStatus::Blocked)
        .count();
    let awaiting_human = state
        .inbox_items
        .iter()
        .filter(|item| item.requires_human_action && item.status != InboxStatus::Done)
        .count();

    let mut needs_action_items = Vec::new();
    let mut in_progress_items = Vec::new();
    let mut blocked_items = Vec::new();
    let mut done_items = Vec::new();
    for item in &state.inbox_items {
        match item.status {
            InboxStatus::New | InboxStatus::Acknowledged => needs_action_items.push(item),
            InboxStatus::InProgress => in_progress_items.push(item),
            InboxStatus::Blocked => blocked_items.push(item),
            InboxStatus::Done | InboxStatus::Dismissed => done_items.push(item),
        }
    }

    let sort_section_by_due = |items: &mut Vec<&InboxItem>| {
        items.sort_by(|a, b| {
            a.due_at
                .or_else(|| infer_due_at_from_item_text(a))
                .unwrap_or(i64::MAX)
                .cmp(
                    &b.due_at
                        .or_else(|| infer_due_at_from_item_text(b))
                        .unwrap_or(i64::MAX),
                )
                .then_with(|| priority_rank(a.priority).cmp(&priority_rank(b.priority)))
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
    };

    sort_section_by_due(&mut needs_action_items);
    sort_section_by_due(&mut in_progress_items);
    sort_section_by_due(&mut blocked_items);
    sort_section_by_due(&mut done_items);

    let content = column![
        row![
            inbox_chip("Actionable now", actionable_now),
            inbox_chip("Overdue", overdue),
            inbox_chip("Blocked", blocked),
            inbox_chip("Awaiting human", awaiting_human),
            Space::new().width(Length::Fill),
            button("Refresh")
                .padding([8, 12])
                .style(rounded_primary_button)
                .on_press_maybe((!state.inbox_refresh_in_flight).then_some(Message::InboxRefreshRequested))
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        container(
            text("Action semantics: Seen = reviewed • Start = in progress • Block = waiting/dependency • Done = completed • Undo = reopen if DoD not met • Snooze = remind later")
                .size(13)
        )
        .padding([8, 10])
        .style(glass_accent_panel),
        if state.inbox_error.is_empty() {
            text(state.inbox_status.clone())
        } else {
            text(state.inbox_error.clone()).color([0.95, 0.45, 0.45])
        },
        inbox_section(
            "Needs Action",
            &needs_action_items,
            state.timeline_focus_origin_ref.as_deref(),
            state.inbox_action_origin_ref_in_flight.as_deref(),
        ),
        inbox_section(
            "In progress",
            &in_progress_items,
            state.timeline_focus_origin_ref.as_deref(),
            state.inbox_action_origin_ref_in_flight.as_deref(),
        ),
        inbox_section(
            "Blocked",
            &blocked_items,
            state.timeline_focus_origin_ref.as_deref(),
            state.inbox_action_origin_ref_in_flight.as_deref(),
        ),
        inbox_section(
            "Done",
            &done_items,
            state.timeline_focus_origin_ref.as_deref(),
            state.inbox_action_origin_ref_in_flight.as_deref(),
        ),
    ]
    .spacing(10)
    .width(Length::Fill);

    container(
        scrollable(container(content).width(Length::Fill).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn inbox_chip<'a>(label: &'a str, value: usize) -> Element<'a, Message> {
    container(text(format!("{label}: {value}")).size(13))
        .padding([6, 10])
        .style(glass_panel)
        .into()
}

#[derive(Clone, Copy)]
enum BadgeTone {
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
    Muted,
}

fn metric_badge(label: &str, value: impl Into<String>) -> Element<'static, Message> {
    metric_badge_tone(label, value, BadgeTone::Neutral)
}

fn metric_badge_tone(
    label: &str,
    value: impl Into<String>,
    tone: BadgeTone,
) -> Element<'static, Message> {
    let style = match tone {
        BadgeTone::Neutral => glass_panel,
        BadgeTone::Info => glass_accent_panel,
        BadgeTone::Success => glass_success_panel,
        BadgeTone::Warning => glass_warning_panel,
        BadgeTone::Danger => glass_alert_panel,
        BadgeTone::Muted => glass_muted_panel,
    };

    container(text(format!("{label}: {}", value.into())).size(11))
        .padding([4, 8])
        .style(style)
        .into()
}

fn format_minutes_short(minutes: i32) -> String {
    let minutes = minutes.max(1);
    if minutes >= 8 * 60 {
        let days = minutes as f32 / (8.0 * 60.0);
        format!("{days:.1}d")
    } else if minutes >= 60 {
        let hours = minutes as f32 / 60.0;
        format!("{hours:.1}h")
    } else {
        format!("{minutes}m")
    }
}

fn inbox_section<'a>(
    title: &'a str,
    items: &[&InboxItem],
    focused_origin_ref: Option<&str>,
    action_in_flight_origin_ref: Option<&str>,
) -> Element<'a, Message> {
    let rows = if items.is_empty() {
        column![container(text("No items")).padding(8).style(glass_panel)]
    } else {
        items.iter().fold(column!().spacing(8), |col, item| {
            let priority = match item.priority {
                InboxPriority::Low => "low",
                InboxPriority::Normal => "normal",
                InboxPriority::High => "high",
                InboxPriority::Urgent => "urgent",
            };
            let source = match item.source_type {
                InboxSourceType::Reminder => "reminder",
                InboxSourceType::Todo => "todo",
                InboxSourceType::Task => "task",
                InboxSourceType::PlanStep => "plan",
            };
            let due_ts = item.due_at.or_else(|| infer_due_at_from_item_text(item));
            let due = due_ts
                .map(format_due_badge_time)
                .unwrap_or_else(|| "no due date".to_string());

            let now = now_unix_ts();
            let due_tone = if let Some(ts) = due_ts {
                if item.status.is_actionable() && ts < now {
                    BadgeTone::Danger
                } else if item.status.is_actionable() && ts <= now + 24 * 60 * 60 {
                    BadgeTone::Warning
                } else {
                    BadgeTone::Success
                }
            } else {
                BadgeTone::Muted
            };

            let mut meta_badges = row![
                metric_badge_tone("Type", source.to_string(), BadgeTone::Info),
                metric_badge("Priority", priority.to_string()),
                metric_badge_tone("Due", due.clone(), due_tone),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);

            if let Some(size) = item.t_shirt_size.as_ref() {
                let size_norm = size.to_ascii_uppercase();
                let size_tone = match size_norm.as_str() {
                    "XS" | "S" => BadgeTone::Success,
                    "M" => BadgeTone::Info,
                    "L" => BadgeTone::Warning,
                    "XL" | "XXL" => BadgeTone::Danger,
                    _ => BadgeTone::Neutral,
                };
                meta_badges = meta_badges.push(metric_badge_tone("Size", size_norm, size_tone));
            }
            if let Some(points) = item.story_points {
                if points > 0 {
                    let points_tone = if points >= 8 {
                        BadgeTone::Danger
                    } else if points >= 5 {
                        BadgeTone::Warning
                    } else {
                        BadgeTone::Success
                    };
                    meta_badges =
                        meta_badges.push(metric_badge_tone("SP", points.to_string(), points_tone));
                }
            }
            if let Some(minutes) = item.estimate_optimistic_minutes {
                meta_badges = meta_badges.push(metric_badge_tone(
                    "Opt",
                    format_minutes_short(minutes),
                    BadgeTone::Info,
                ));
            }
            if let Some(minutes) = item.estimate_likely_minutes {
                meta_badges = meta_badges.push(metric_badge_tone(
                    "Likely",
                    format_minutes_short(minutes),
                    BadgeTone::Warning,
                ));
            }
            if let Some(minutes) = item.estimate_pessimistic_minutes {
                meta_badges = meta_badges.push(metric_badge_tone(
                    "Pess",
                    format_minutes_short(minutes),
                    BadgeTone::Danger,
                ));
            }

            let row_in_flight = action_in_flight_origin_ref == Some(item.origin_ref.as_str());
            let can_transition = item.status.is_actionable() && !row_in_flight;
            let can_seen = can_transition && item.status == InboxStatus::New;
            let can_start = can_transition
                && matches!(
                    item.status,
                    InboxStatus::New | InboxStatus::Acknowledged | InboxStatus::Blocked
                );
            let can_block = can_transition
                && matches!(
                    item.status,
                    InboxStatus::New | InboxStatus::Acknowledged | InboxStatus::InProgress
                );
            let can_done = can_transition
                && matches!(
                    item.status,
                    InboxStatus::New
                        | InboxStatus::Acknowledged
                        | InboxStatus::InProgress
                        | InboxStatus::Blocked
                );
            let can_reopen = !row_in_flight && item.status == InboxStatus::Done;
            let can_snooze = can_transition
                && item.source_type == InboxSourceType::Reminder
                && matches!(
                    item.status,
                    InboxStatus::New
                        | InboxStatus::Acknowledged
                        | InboxStatus::InProgress
                        | InboxStatus::Blocked
                );
            let action_row = row![
                button("Seen")
                    .padding([6, 10])
                    .style(rounded_secondary_button)
                    .on_press_maybe(
                        can_seen.then_some(Message::InboxAcknowledge(item.origin_ref.clone()))
                    ),
                button("Start")
                    .padding([6, 10])
                    .style(rounded_primary_button)
                    .on_press_maybe(
                        can_start.then_some(Message::InboxStart(item.origin_ref.clone()))
                    ),
                button("Blocked")
                    .padding([6, 10])
                    .style(rounded_secondary_button)
                    .on_press_maybe(
                        can_block.then_some(Message::InboxBlock(item.origin_ref.clone()))
                    ),
                button("Done")
                    .padding([6, 10])
                    .style(rounded_success_button)
                    .on_press_maybe(
                        can_done.then_some(Message::InboxDone(item.origin_ref.clone()))
                    ),
                button("Undo")
                    .padding([6, 10])
                    .style(rounded_secondary_button)
                    .on_press_maybe(
                        can_reopen.then_some(Message::InboxReopen(item.origin_ref.clone()))
                    ),
                button("Snooze")
                    .padding([6, 10])
                    .style(rounded_secondary_button)
                    .on_press_maybe(
                        can_snooze.then_some(Message::InboxSnooze(item.origin_ref.clone()))
                    ),
            ]
            .spacing(8);

            col.push(
                container(
                    column![
                        row![
                            text(if item.status == InboxStatus::New {
                                "●"
                            } else {
                                "○"
                            })
                            .size(15)
                            .color(
                                if item.status == InboxStatus::New {
                                    Color::from_rgb(0.30, 0.66, 1.0)
                                } else {
                                    Color::from_rgb(0.60, 0.64, 0.72)
                                }
                            ),
                            Space::new().width(8),
                            text(item.title.clone()).size(16),
                            Space::new().width(Length::Fill),
                            text(inbox_status_label(item.status)).size(12)
                        ]
                        .align_y(iced::Alignment::Center),
                        if let Some(details) = &item.details {
                            text(details.clone()).size(13)
                        } else {
                            text("").size(1)
                        },
                        scrollable(meta_badges)
                            .direction(iced::widget::scrollable::Direction::Horizontal(
                                iced::widget::scrollable::Scrollbar::default(),
                            ))
                            .height(Length::Shrink)
                            .width(Length::Fill),
                        text(format!(
                            "Ref: {} • Updated: {}",
                            item.id,
                            format_local_time(item.updated_at)
                        ))
                        .size(12),
                        action_row,
                        if row_in_flight {
                            text("Updating...").size(12).color([0.70, 0.86, 1.0])
                        } else {
                            text("").size(1)
                        },
                    ]
                    .spacing(6),
                )
                .padding(10)
                .style(if focused_origin_ref == Some(item.origin_ref.as_str()) {
                    glass_accent_panel
                } else {
                    glass_panel
                }),
            )
        })
    };

    container(
        column![
            row![
                text(title.to_uppercase()).size(22),
                Space::new().width(Length::Fill),
                inbox_chip("items", items.len()),
            ]
            .align_y(iced::Alignment::Center),
            rows
        ]
        .spacing(8),
    )
    .padding(8)
    .style(glass_panel)
    .into()
}

#[allow(dead_code)]
fn view_timeline_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let mut human_lane: Vec<&InboxItem> = vec![];
    let mut agent_lane: Vec<&InboxItem> = vec![];
    for item in &state.inbox_items {
        if item.owner.eq_ignore_ascii_case("agent") {
            agent_lane.push(item);
        } else {
            human_lane.push(item);
        }
    }

    let human_blocked = human_lane
        .iter()
        .filter(|item| item.status == InboxStatus::Blocked)
        .count();
    let agent_blocked = agent_lane
        .iter()
        .filter(|item| item.status == InboxStatus::Blocked)
        .count();

    let mut critical_paths = Vec::new();
    let item_index: HashMap<&str, &InboxItem> = state
        .inbox_items
        .iter()
        .map(|item| (item.origin_ref.as_str(), item))
        .collect();

    for item in state
        .inbox_items
        .iter()
        .filter(|item| item.status == InboxStatus::Blocked)
    {
        let mut visited = HashSet::new();
        let mut chain = vec![item.title.clone()];
        let mut current = item;
        visited.insert(current.origin_ref.clone());
        let mut unresolved = false;

        for _ in 0..4 {
            let Some(dep_ref) = current.dependency_refs.first() else {
                break;
            };

            if !visited.insert(dep_ref.clone()) {
                unresolved = true;
                chain.push("cycle detected".to_string());
                break;
            }

            if let Some(next) = item_index.get(dep_ref.as_str()) {
                chain.push(next.title.clone());
                current = next;
            } else {
                unresolved = true;
                chain.push(format!("missing dependency ({dep_ref})"));
                break;
            }
        }

        critical_paths.push((
            item.origin_ref.clone(),
            item.priority,
            item.due_at.unwrap_or(i64::MAX),
            chain.join("  →  "),
            unresolved,
        ));
    }

    critical_paths.sort_by_key(|entry| (priority_rank(entry.1), entry.2));
    if critical_paths.len() > 8 {
        critical_paths.truncate(8);
    }

    let critical_path_cards = if critical_paths.is_empty() {
        column![
            container(text("No blocked dependency chains right now").size(13))
                .padding([10, 12])
                .style(glass_panel)
        ]
    } else {
        critical_paths.into_iter().fold(
            column!().spacing(8),
            |col, (origin_ref, _, _, chain, unresolved)| {
                col.push(
                    container(
                        column![
                            row![
                                text(if unresolved { "⚠" } else { "⛓" }).size(15),
                                text(chain).size(13),
                            ]
                            .spacing(8)
                            .align_y(iced::Alignment::Center),
                            row![
                                text(origin_ref.clone()).size(11),
                                Space::new().width(Length::Fill),
                                button("Open item")
                                    .padding([4, 10])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::TimelineOpenItem(origin_ref.clone())),
                                button("Open audit")
                                    .padding([4, 10])
                                    .style(rounded_primary_button)
                                    .on_press(Message::TimelineOpenAudit(origin_ref.clone())),
                                button("Open chat")
                                    .padding([4, 10])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::OpenChatWithContext(origin_ref.clone())),
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                        ]
                        .spacing(6),
                    )
                    .padding([8, 10])
                    .style(if unresolved {
                        glass_alert_panel
                    } else {
                        glass_panel
                    }),
                )
            },
        )
    };

    let lane = |title: &'static str, mut items: Vec<&InboxItem>| {
        let lane_blocked = items
            .iter()
            .filter(|item| item.status == InboxStatus::Blocked)
            .count();
        items.sort_by_key(|item| {
            (
                item.status != InboxStatus::Blocked,
                item.due_at.unwrap_or(i64::MAX),
                item.created_at,
            )
        });

        let rows = if items.is_empty() {
            column![container(text("No items in this lane").size(13))
                .padding([10, 12])
                .style(glass_panel)]
        } else {
            items.into_iter().fold(column!().spacing(8), |col, item| {
                let due = item
                    .due_at
                    .map(format_local_time)
                    .unwrap_or_else(|| "no due date".to_string());
                let status = inbox_status_label(item.status);
                let status_color = inbox_status_color(item.status);
                let priority = match item.priority {
                    InboxPriority::Low => "low",
                    InboxPriority::Normal => "normal",
                    InboxPriority::High => "high",
                    InboxPriority::Urgent => "urgent",
                };
                let owner = if item.owner.eq_ignore_ascii_case("agent") {
                    "agent"
                } else {
                    "human"
                };

                let mut badges = row![
                    metric_badge("Owner", owner.to_string()),
                    metric_badge("Priority", priority.to_string()),
                    metric_badge("Due", due.clone()),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center);
                if let Some(size) = item.t_shirt_size.as_ref() {
                    badges = badges.push(metric_badge("Size", size.to_ascii_uppercase()));
                }
                if let Some(points) = item.story_points {
                    if points > 0 {
                        badges = badges.push(metric_badge("SP", points.to_string()));
                    }
                }
                if let Some(minutes) = item.estimate_likely_minutes {
                    badges = badges.push(metric_badge("Likely", format_minutes_short(minutes)));
                }

                col.push(
                    container(
                        column![
                            row![
                                text(item.title.clone()).size(15),
                                Space::new().width(Length::Fill),
                                text("●").size(13).color(status_color),
                                text(status).size(12).color(status_color)
                            ]
                            .align_y(iced::Alignment::Center),
                            scrollable(badges)
                                .direction(iced::widget::scrollable::Direction::Horizontal(
                                    iced::widget::scrollable::Scrollbar::default(),
                                ))
                                .height(Length::Shrink)
                                .width(Length::Fill),
                        ]
                        .spacing(6),
                    )
                    .padding([10, 12])
                    .style(if item.status == InboxStatus::Blocked {
                        glass_alert_panel
                    } else {
                        glass_panel
                    }),
                )
            })
        };

        container(
            column![
                row![
                    text(title).size(18),
                    Space::new().width(Length::Fill),
                    inbox_chip("blocked", lane_blocked),
                ]
                .align_y(iced::Alignment::Center),
                rows
            ]
            .spacing(8),
        )
        .padding(8)
        .style(glass_panel)
    };

    let total_overdue = state
        .inbox_items
        .iter()
        .filter(|item| item.due_at.map(|due| due < now_unix_ts()).unwrap_or(false))
        .count();

    let content = column![
        row![
            inbox_chip("Human blocked", human_blocked),
            inbox_chip("Agent blocked", agent_blocked),
            inbox_chip("Overdue", total_overdue),
            inbox_chip("Total items", state.inbox_items.len()),
            Space::new().width(Length::Fill),
            button("Refresh")
                .padding([8, 12])
                .style(rounded_primary_button)
                .on_press(Message::InboxRefreshRequested),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        container(
            row![
                text("Blocker-first timeline").size(14),
                Space::new().width(Length::Fill),
                text("Human + Agent lanes sorted by blocked status and due time").size(12)
            ]
            .align_y(iced::Alignment::Center)
        )
        .padding([10, 12])
        .style(glass_accent_panel),
        container(
            column![
                row![
                    text("Critical path").size(16),
                    Space::new().width(Length::Fill),
                    text("Blocked chains surfaced first").size(12),
                ]
                .align_y(iced::Alignment::Center),
                critical_path_cards,
            ]
            .spacing(8)
        )
        .padding(8)
        .style(glass_panel),
        lane("Human lane", human_lane),
        lane("Agent lane", agent_lane),
    ]
    .spacing(10);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_kanban_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let now = now_unix_ts();
    let seven_days_ago = now - (7 * 24 * 60 * 60);

    let mut new_items = Vec::new();
    let mut acknowledged_items = Vec::new();
    let mut in_progress_items = Vec::new();
    let mut blocked_items = Vec::new();
    let mut done_items = Vec::new();

    for item in &state.inbox_items {
        match item.status {
            InboxStatus::New => new_items.push(item),
            InboxStatus::Acknowledged => acknowledged_items.push(item),
            InboxStatus::InProgress => in_progress_items.push(item),
            InboxStatus::Blocked => blocked_items.push(item),
            InboxStatus::Done | InboxStatus::Dismissed => done_items.push(item),
        }
    }

    let completed_with_due = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed)
                && item.due_at.is_some()
        })
        .count();
    let completed_on_time = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed)
                && item
                    .due_at
                    .map(|due| item.updated_at <= due)
                    .unwrap_or(false)
        })
        .count();
    let deadline_hit_rate_pct = if completed_with_due > 0 {
        ((completed_on_time as f32 / completed_with_due as f32) * 100.0).round() as i32
    } else {
        0
    };

    let throughput_7d = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed)
                && item.updated_at >= seven_days_ago
        })
        .count();

    let velocity_points_7d: i32 = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed)
                && item.updated_at >= seven_days_ago
        })
        .map(|item| item.story_points.unwrap_or(0).max(0))
        .sum();

    let active_count =
        new_items.len() + acknowledged_items.len() + in_progress_items.len() + blocked_items.len();
    let done_count = done_items.len();
    let done_open_ratio = if active_count > 0 {
        format!("{:.1}", done_count as f32 / active_count as f32)
    } else {
        "∞".to_string()
    };

    let overdue_open = state
        .inbox_items
        .iter()
        .filter(|item| {
            !matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed)
                && item.due_at.map(|due| due < now).unwrap_or(false)
        })
        .count();

    let item_index: HashMap<&str, &InboxItem> = state
        .inbox_items
        .iter()
        .map(|item| (item.origin_ref.as_str(), item))
        .collect();
    let blocked_with_dependencies = blocked_items
        .iter()
        .filter(|item| !item.dependency_refs.is_empty())
        .count();
    let unresolved_dependency_edges = blocked_items
        .iter()
        .flat_map(|item| item.dependency_refs.iter())
        .filter(|dep_ref| !item_index.contains_key(dep_ref.as_str()))
        .count();

    let mut dependency_chains = blocked_items
        .iter()
        .filter_map(|item| {
            let dep_ref = item.dependency_refs.first()?;
            let dep_title = item_index
                .get(dep_ref.as_str())
                .map(|dep| dep.title.clone())
                .unwrap_or_else(|| format!("missing ({dep_ref})"));
            Some((
                item.origin_ref.clone(),
                format!("{} → {}", item.title, dep_title),
            ))
        })
        .collect::<Vec<_>>();
    dependency_chains.sort_by(|a, b| a.1.cmp(&b.1));
    dependency_chains.truncate(3);

    let dependency_chain_preview: Element<'_, Message> = if dependency_chains.is_empty() {
        container(text("No blocked dependency chains right now").size(12))
            .padding([8, 10])
            .style(glass_panel)
            .width(Length::Fill)
            .into()
    } else {
        dependency_chains
            .into_iter()
            .fold(column!().spacing(6), |col, (origin_ref, chain)| {
                col.push(
                    container(
                        column![
                            text(chain).size(12),
                            row![
                                Space::new().width(Length::Fill),
                                button("Inbox")
                                    .padding([4, 8])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::TimelineOpenItem(origin_ref.clone())),
                                button("Audit")
                                    .padding([4, 8])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::TimelineOpenAudit(origin_ref.clone())),
                                button("Chat")
                                    .padding([4, 8])
                                    .style(rounded_primary_button)
                                    .on_press(Message::OpenChatWithContext(origin_ref.clone())),
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                        ]
                        .spacing(6),
                    )
                    .padding([8, 10])
                    .style(glass_panel)
                    .width(Length::Fill),
                )
            })
            .into()
    };

    let column_view = |title: &'static str, items: Vec<&InboxItem>| {
        let max_cards = 4usize;
        let item_count = items.len();
        let cards = if items.is_empty() {
            column![container(text("No items").size(12))
                .padding([8, 10])
                .style(glass_panel)
                .width(Length::Fill)]
        } else {
            let mut cards = items.into_iter().take(max_cards).fold(
                column!().spacing(8),
                |col, item| {
                    let due = item
                        .due_at
                        .map(format_due_badge_time)
                        .unwrap_or_else(|| "no due".to_string());
                    let source = match item.source_type {
                        InboxSourceType::Reminder => "reminder",
                        InboxSourceType::Todo => "todo",
                        InboxSourceType::Task => "task",
                        InboxSourceType::PlanStep => "plan",
                    };
                    let size = item
                        .t_shirt_size
                        .clone()
                        .unwrap_or_else(|| "-".to_string())
                        .to_ascii_uppercase();
                    let points = item.story_points.unwrap_or(0).max(0);
                    let likely = item
                        .estimate_likely_minutes
                        .map(format_minutes_short)
                        .unwrap_or_else(|| "-".to_string());

                    col.push(
                        container(
                            column![
                                text(item.title.clone()).size(14),
                                text(format!("{source} • {}", item.owner)).size(12),
                                text(format!(
                                    "due: {due} • size: {size} • sp: {points} • likely: {likely}"
                                ))
                                .size(11),
                                text(item.origin_ref.to_string()).size(11),
                            ]
                            .spacing(4),
                        )
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(if item.status == InboxStatus::Blocked {
                            glass_alert_panel
                        } else {
                            glass_panel
                        }),
                    )
                },
            );

            let overflow = item_count.saturating_sub(max_cards);
            if overflow > 0 {
                cards = cards.push(
                    container(
                        text(format!("+{overflow} more items (see Inbox)"))
                            .size(12)
                            .color([0.78, 0.82, 0.92]),
                    )
                    .padding([8, 10])
                    .style(glass_muted_panel)
                    .width(Length::Fill),
                );
            }

            cards
        };

        container(
            column![
                row![
                    text(title).size(18),
                    Space::new().width(Length::Fill),
                    inbox_chip("items", item_count),
                ]
                .align_y(iced::Alignment::Center),
                cards
            ]
            .spacing(8),
        )
        .padding(8)
        .style(glass_panel)
        .width(Length::FillPortion(1))
    };

    let board = row![
        column_view("NEW", new_items),
        column_view("SEEN", acknowledged_items),
        column_view("IN PROGRESS", in_progress_items),
        column_view("BLOCKED", blocked_items),
        column_view("DONE", done_items),
    ]
    .spacing(10)
    .height(Length::Fill)
    .width(Length::Fill);

    let content = column![
        container(
            row![
                text("Kanban board + delivery metrics").size(14),
                Space::new().width(Length::Fill),
                text("No inline edits in this view").size(12),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 10])
        .style(glass_accent_panel),
        row![
            metric_badge(
                "On-time",
                format!("{}%", deadline_hit_rate_pct.clamp(0, 100))
            ),
            metric_badge("Overdue open", overdue_open.to_string()),
            metric_badge("Throughput (7d)", throughput_7d.to_string()),
            metric_badge("Velocity SP (7d)", velocity_points_7d.max(0).to_string()),
            metric_badge("Done/Open", done_open_ratio),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        container(
            column![
                row![
                    text("Dependency watch").size(15),
                    Space::new().width(Length::Fill),
                    text("Lightweight graph preview").size(12),
                ]
                .align_y(iced::Alignment::Center),
                row![
                    inbox_chip("Blocked w/ deps", blocked_with_dependencies),
                    inbox_chip("Unresolved edges", unresolved_dependency_edges),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                dependency_chain_preview
            ]
            .spacing(8)
        )
        .padding(8)
        .style(glass_panel),
        board,
    ]
    .spacing(10)
    .height(Length::Fill);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_dependencies_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let item_index: HashMap<&str, &InboxItem> = state
        .inbox_items
        .iter()
        .map(|item| (item.origin_ref.as_str(), item))
        .collect();

    let mut edges = state
        .inbox_items
        .iter()
        .flat_map(|item| {
            item.dependency_refs.iter().map(|dep_ref| {
                let dep_item = item_index.get(dep_ref.as_str()).copied();
                (
                    item.origin_ref.clone(),
                    item.title.clone(),
                    item.owner.clone(),
                    item.status,
                    dep_ref.clone(),
                    dep_item.map(|dep| dep.title.clone()),
                    dep_item.map(|dep| dep.owner.clone()),
                    dep_item.is_some(),
                )
            })
        })
        .collect::<Vec<_>>();

    edges.sort_by(|a, b| {
        let a_unresolved = !a.7;
        let b_unresolved = !b.7;
        a_unresolved
            .cmp(&b_unresolved)
            .reverse()
            .then_with(|| (a.3 != InboxStatus::Blocked).cmp(&(b.3 != InboxStatus::Blocked)))
            .then_with(|| a.1.cmp(&b.1))
    });

    let total_edges = edges.len();
    let unresolved_edges = edges.iter().filter(|edge| !edge.7).count();
    let blocked_edges = edges
        .iter()
        .filter(|edge| edge.3 == InboxStatus::Blocked)
        .count();
    let cross_owner_edges = edges
        .iter()
        .filter(|edge| {
            edge.6
                .as_ref()
                .map(|dep_owner| !edge.2.eq_ignore_ascii_case(dep_owner))
                .unwrap_or(false)
        })
        .count();
    let cross_owner_blocked_edges = edges
        .iter()
        .filter(|edge| {
            edge.3 == InboxStatus::Blocked
                && edge
                    .6
                    .as_ref()
                    .map(|dep_owner| !edge.2.eq_ignore_ascii_case(dep_owner))
                    .unwrap_or(false)
        })
        .count();

    let mut owner_pair_rollups: HashMap<String, (usize, usize, usize)> = HashMap::new();
    for edge in &edges {
        let source_owner = edge.2.trim().to_ascii_lowercase();
        let dep_owner = edge
            .6
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        let key = format!("{source_owner} → {dep_owner}");
        let entry = owner_pair_rollups.entry(key).or_insert((0, 0, 0));
        entry.0 += 1;
        if edge.3 == InboxStatus::Blocked {
            entry.1 += 1;
        }
        if !edge.7 {
            entry.2 += 1;
        }
    }

    let mut owner_pair_rows = owner_pair_rollups.into_iter().collect::<Vec<_>>();
    owner_pair_rows.sort_by(|a, b| {
        b.1 .2
            .cmp(&a.1 .2)
            .then_with(|| b.1 .1.cmp(&a.1 .1))
            .then_with(|| b.1 .0.cmp(&a.1 .0))
            .then_with(|| a.0.cmp(&b.0))
    });
    owner_pair_rows.truncate(8);

    let mut cycle_count = 0usize;
    for item in &state.inbox_items {
        for dep_ref in &item.dependency_refs {
            if let Some(dep_item) = item_index.get(dep_ref.as_str()) {
                if dep_item
                    .dependency_refs
                    .iter()
                    .any(|back_ref| back_ref == &item.origin_ref)
                {
                    cycle_count += 1;
                }
            }
        }
    }

    let graph_rows = if edges.is_empty() {
        column![container(text("No dependency edges yet.").size(13))
            .padding([10, 12])
            .style(glass_panel)
            .width(Length::Fill)]
    } else {
        edges
            .iter()
            .take(40)
            .fold(column!().spacing(8), |col, edge| {
                let source_ref = edge.0.clone();
                let from_title = edge.1.clone();
                let status = edge.3;
                let dep_ref = edge.4.clone();
                let dep_title = edge
                    .5
                    .clone()
                    .unwrap_or_else(|| format!("missing dependency ({dep_ref})"));
                let unresolved = !edge.7;

                let owner_pair = match edge.6.clone() {
                    Some(dep_owner) => format!("{} → {dep_owner}", edge.2),
                    None => format!("{} → unknown", edge.2),
                };

                col.push(
                    container(
                        column![
                            row![
                                text(if unresolved { "⚠" } else { "⛓" }).size(14),
                                text(format!("{from_title} → {dep_title}")).size(13),
                                Space::new().width(Length::Fill),
                                text(inbox_status_label(status)).size(12),
                            ]
                            .spacing(8)
                            .align_y(iced::Alignment::Center),
                            row![
                                text(owner_pair).size(11),
                                Space::new().width(Length::Fill),
                                button("Inbox")
                                    .padding([4, 8])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::TimelineOpenItem(source_ref.clone())),
                                button("Audit")
                                    .padding([4, 8])
                                    .style(rounded_secondary_button)
                                    .on_press(Message::TimelineOpenAudit(source_ref.clone())),
                                button("Chat")
                                    .padding([4, 8])
                                    .style(rounded_primary_button)
                                    .on_press(Message::OpenChatWithContext(source_ref.clone())),
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                        ]
                        .spacing(6),
                    )
                    .padding([8, 10])
                    .style(if unresolved {
                        glass_alert_panel
                    } else if status == InboxStatus::Blocked {
                        glass_warning_panel
                    } else {
                        glass_panel
                    }),
                )
            })
    };

    let owner_rollup_rows: Element<'_, Message> = if owner_pair_rows.is_empty() {
        container(text("No owner handoffs detected yet.").size(12))
            .padding([8, 10])
            .style(glass_panel)
            .width(Length::Fill)
            .into()
    } else {
        owner_pair_rows
            .into_iter()
            .fold(
                column!().spacing(6),
                |col, (pair, (total, blocked, unresolved))| {
                    col.push(
                        container(
                            row![
                                text(pair).size(12),
                                Space::new().width(Length::Fill),
                                metric_badge("Edges", total.to_string()),
                                metric_badge_tone(
                                    "Blocked",
                                    blocked.to_string(),
                                    if blocked > 0 {
                                        BadgeTone::Warning
                                    } else {
                                        BadgeTone::Muted
                                    }
                                ),
                                metric_badge_tone(
                                    "Unresolved",
                                    unresolved.to_string(),
                                    if unresolved > 0 {
                                        BadgeTone::Danger
                                    } else {
                                        BadgeTone::Success
                                    }
                                ),
                            ]
                            .spacing(6)
                            .align_y(iced::Alignment::Center),
                        )
                        .padding([8, 10])
                        .style(glass_panel)
                        .width(Length::Fill),
                    )
                },
            )
            .into()
    };

    let content = column![
        container(
            row![
                text("Dependency graph").size(14),
                Space::new().width(Length::Fill),
                text("Edges across todos/plans/tasks/reminders").size(12),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 10])
        .style(glass_accent_panel),
        row![
            inbox_chip("Total edges", total_edges),
            inbox_chip("Unresolved", unresolved_edges),
            inbox_chip("Blocked edges", blocked_edges),
            inbox_chip("Cross-owner", cross_owner_edges),
            inbox_chip("Cross-owner blocked", cross_owner_blocked_edges),
            inbox_chip("2-node cycles", cycle_count),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        container(
            column![
                row![
                    text("Org handoff rollup").size(15),
                    Space::new().width(Length::Fill),
                    text("owner → dependency-owner edges").size(12),
                ]
                .align_y(iced::Alignment::Center),
                owner_rollup_rows,
            ]
            .spacing(8),
        )
        .padding(8)
        .style(glass_panel),
        graph_rows,
    ]
    .spacing(10)
    .width(Length::Fill);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_gantt_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let extract_plan_step_ref = |details: Option<&String>| -> Option<String> {
        static PLAN_STEP_REF_RE: OnceLock<Regex> = OnceLock::new();
        let text = details?.as_str();
        let re = PLAN_STEP_REF_RE
            .get_or_init(|| Regex::new(r"(?i)planstepref\s*:\s*(plan_step:\d+:\d+)").unwrap());
        re.captures(text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_ascii_lowercase())
    };

    let plan_step_refs_in_view = state
        .inbox_items
        .iter()
        .filter(|item| item.source_type == InboxSourceType::PlanStep)
        .map(|item| item.origin_ref.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let mut gantt_rows = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(
                item.source_type,
                InboxSourceType::Todo | InboxSourceType::PlanStep
            ) && (item.due_at.is_some() || item.estimate_likely_minutes.is_some())
                && !(item.source_type == InboxSourceType::Todo
                    && extract_plan_step_ref(item.details.as_ref())
                        .map(|origin_ref| plan_step_refs_in_view.contains(&origin_ref))
                        .unwrap_or(false))
        })
        .collect::<Vec<_>>();

    let item_index: HashMap<String, &InboxItem> = state
        .inbox_items
        .iter()
        .map(|item| (item.origin_ref.to_ascii_lowercase(), item))
        .collect();

    let mut seen_fingerprints = HashSet::new();
    gantt_rows.retain(|item| {
        let due_bucket = item
            .due_at
            .map(|ts| ts / 86_400)
            .unwrap_or(i64::MAX / 86_400);
        let fingerprint = format!(
            "{}|{}|{}|{}|{}",
            item.title.trim().to_ascii_lowercase(),
            due_bucket,
            item.story_points.unwrap_or(0),
            item.estimate_likely_minutes.unwrap_or(0),
            item.t_shirt_size
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        );

        if seen_fingerprints.contains(&fingerprint) {
            return false;
        }
        seen_fingerprints.insert(fingerprint)
    });

    let gantt_start_end = |item: &InboxItem| -> (i64, i64) {
        let likely_seconds = (item.estimate_likely_minutes.unwrap_or(60).max(1) as i64) * 60;
        let end_ts = item
            .due_at
            .unwrap_or_else(|| item.created_at + likely_seconds)
            .max(item.created_at + 60);

        let latest_dep_end = item
            .dependency_refs
            .iter()
            .filter_map(|dep_ref| item_index.get(&dep_ref.to_ascii_lowercase()).copied())
            .map(|dep| {
                dep.due_at.unwrap_or_else(|| {
                    dep.created_at + (dep.estimate_likely_minutes.unwrap_or(60).max(1) as i64 * 60)
                })
            })
            .max();

        let start_from_due = item
            .due_at
            .map(|due| (due - likely_seconds).max(0))
            .unwrap_or(item.created_at);

        let start_ts = latest_dep_end
            .filter(|dep_end| *dep_end < end_ts)
            .unwrap_or(start_from_due)
            .min(end_ts - 60);
        (start_ts, end_ts)
    };

    gantt_rows.sort_by_key(|item| {
        let (start_ts, end_ts) = gantt_start_end(item);
        (start_ts, end_ts, item.created_at)
    });

    let time_window_start = gantt_rows
        .iter()
        .map(|item| gantt_start_end(item).0)
        .min()
        .unwrap_or_else(now_unix_ts);
    let time_window_end = gantt_rows
        .iter()
        .map(|item| gantt_start_end(item).1)
        .max()
        .unwrap_or(time_window_start + 3600)
        .max(time_window_start + 3600);
    let total_window_seconds = (time_window_end - time_window_start).max(1) as f32;

    let rows = if gantt_rows.is_empty() {
        column![container(text("No todo tickets available for Gantt view"))
            .padding([10, 12])
            .style(glass_panel)]
    } else {
        gantt_rows
            .into_iter()
            .fold(column!().spacing(10), |col, item| {
                let (start_ts, end_ts) = gantt_start_end(item);

                let offset_px =
                    (((start_ts - time_window_start).max(0) as f32) / total_window_seconds * 560.0)
                        .clamp(0.0, 560.0);
                let bar_px = (((end_ts - start_ts).max(60) as f32) / total_window_seconds * 560.0)
                    .clamp(6.0, 560.0);

                let size = item.t_shirt_size.clone().unwrap_or_else(|| "-".to_string());
                let points = item.story_points.unwrap_or(0).max(0);
                let likely_minutes = item.estimate_likely_minutes.unwrap_or(60).max(1);
                let due_label = item
                    .due_at
                    .map(format_local_time)
                    .unwrap_or_else(|| "no due date".to_string());

                col.push(
                    container(
                        column![
                            row![
                                text(item.title.clone()).size(14),
                                Space::new().width(Length::Fill),
                                text(format!("size: {size} • points: {points}")).size(12),
                            ]
                            .align_y(iced::Alignment::Center),
                            row![
                                Space::new().width(offset_px),
                                container(Space::new().width(bar_px).height(14)).style(
                                    if matches!(
                                        item.status,
                                        InboxStatus::Done | InboxStatus::Dismissed
                                    ) {
                                        glass_success_panel
                                    } else if item.status == InboxStatus::Blocked {
                                        glass_alert_panel
                                    } else {
                                        glass_accent_panel
                                    }
                                ),
                                Space::new().width(Length::Fill),
                            ]
                            .spacing(0)
                            .align_y(iced::Alignment::Center),
                            text(format!(
                                "start {} • due {} • likely {} • ref {}",
                                format_local_time(start_ts),
                                due_label,
                                format_minutes_short(likely_minutes),
                                item.origin_ref
                            ))
                            .size(11),
                        ]
                        .spacing(6),
                    )
                    .padding([8, 10])
                    .style(glass_panel),
                )
            })
    };

    let content = column![
        container(
            row![
                text("Read-only Gantt").size(14),
                Space::new().width(Length::Fill),
                text("Blue=open • Green=done • Red=blocked (due-date aligned)").size(12),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 10])
        .style(glass_accent_panel),
        rows,
    ]
    .spacing(10)
    .width(Length::Fill);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[allow(dead_code)]
fn view_burndown_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let extract_plan_step_ref = |details: Option<&String>| -> Option<String> {
        static PLAN_STEP_REF_RE: OnceLock<Regex> = OnceLock::new();
        let text = details?.as_str();
        let re = PLAN_STEP_REF_RE
            .get_or_init(|| Regex::new(r"(?i)planstepref\s*:\s*(plan_step:\d+:\d+)").unwrap());
        re.captures(text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_ascii_lowercase())
    };

    let plan_step_refs_in_view = state
        .inbox_items
        .iter()
        .filter(|item| item.source_type == InboxSourceType::PlanStep)
        .map(|item| item.origin_ref.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let mut burn_items = state
        .inbox_items
        .iter()
        .filter(|item| {
            matches!(
                item.source_type,
                InboxSourceType::Todo | InboxSourceType::PlanStep
            ) && (item.story_points.is_some() || item.estimate_likely_minutes.is_some())
                && !(item.source_type == InboxSourceType::Todo
                    && extract_plan_step_ref(item.details.as_ref())
                        .map(|origin_ref| plan_step_refs_in_view.contains(&origin_ref))
                        .unwrap_or(false))
        })
        .collect::<Vec<_>>();

    let mut seen_fingerprints = HashSet::new();
    burn_items.retain(|item| {
        let due_bucket = item
            .due_at
            .map(|ts| ts / 86_400)
            .unwrap_or(i64::MAX / 86_400);
        let fingerprint = format!(
            "{}|{}|{}|{}|{}",
            item.title.trim().to_ascii_lowercase(),
            due_bucket,
            item.story_points.unwrap_or(0),
            item.estimate_likely_minutes.unwrap_or(0),
            item.t_shirt_size
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        );

        if seen_fingerprints.contains(&fingerprint) {
            return false;
        }
        seen_fingerprints.insert(fingerprint)
    });

    let total_points: i32 = burn_items
        .iter()
        .map(|item| item.story_points.unwrap_or(1).max(1))
        .sum();
    let completed_points: i32 = burn_items
        .iter()
        .filter(|item| matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed))
        .map(|item| item.story_points.unwrap_or(1).max(1))
        .sum();
    let remaining_points = (total_points - completed_points).max(0);

    let total_likely_minutes: i32 = burn_items
        .iter()
        .map(|item| item.estimate_likely_minutes.unwrap_or(60).max(1))
        .sum();
    let completed_likely_minutes: i32 = burn_items
        .iter()
        .filter(|item| matches!(item.status, InboxStatus::Done | InboxStatus::Dismissed))
        .map(|item| item.estimate_likely_minutes.unwrap_or(60).max(1))
        .sum();
    let remaining_likely_minutes = (total_likely_minutes - completed_likely_minutes).max(0);

    let progress_ratio = if total_points > 0 {
        completed_points as f32 / total_points as f32
    } else {
        0.0
    };
    let ideal_remaining_points = ((1.0 - progress_ratio) * total_points as f32).round() as i32;

    let progress_bar = row![
        container(
            Space::new()
                .width((progress_ratio.clamp(0.0, 1.0) * 560.0).max(4.0))
                .height(14)
        )
        .style(glass_accent_panel),
        container(
            Space::new()
                .width(((1.0 - progress_ratio.clamp(0.0, 1.0)) * 560.0).max(4.0))
                .height(14)
        )
        .style(glass_panel),
    ]
    .spacing(0)
    .align_y(iced::Alignment::Center);

    let content = column![
        container(
            row![
                text("Read-only Burndown").size(14),
                Space::new().width(Length::Fill),
                text("Uses sized tickets (Todo + Plan steps)").size(12),
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding([8, 10])
        .style(glass_accent_panel),
        row![
            inbox_chip("Total points", total_points.max(0) as usize),
            inbox_chip("Completed points", completed_points.max(0) as usize),
            inbox_chip("Remaining points", remaining_points.max(0) as usize),
            inbox_chip("Ideal remaining", ideal_remaining_points.max(0) as usize),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        row![
            inbox_chip("Total likely (min)", total_likely_minutes.max(0) as usize),
            inbox_chip(
                "Completed likely (min)",
                completed_likely_minutes.max(0) as usize
            ),
            inbox_chip(
                "Remaining likely (min)",
                remaining_likely_minutes.max(0) as usize
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        container(progress_bar).padding([8, 10]).style(glass_panel),
        container(
            text("Interpretation: left segment = burned work, right segment = remaining work.")
                .size(12)
        )
        .padding([8, 10])
        .style(glass_panel),
    ]
    .spacing(10)
    .width(Length::Fill);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_audit_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let filtered_events = state
        .audit_events
        .iter()
        .filter(|event| {
            if let Some(origin_ref) = state.audit_origin_filter.as_ref() {
                event.origin_ref.as_deref() == Some(origin_ref.as_str())
            } else {
                true
            }
        })
        .cloned()
        .collect::<Vec<_>>();

    let lines = filtered_events
        .iter()
        .fold(column!().spacing(6), |col, event| {
            let (icon, tone, panel) = if event.line.contains("error")
                || event.line.contains("failed")
                || event.line.contains("blocked")
            {
                (
                    "⛔",
                    Color::from_rgb(0.98, 0.58, 0.58),
                    glass_alert_panel as fn(&Theme) -> _,
                )
            } else if event.line.contains("ok") || event.line.contains("done") {
                (
                    "✅",
                    Color::from_rgb(0.62, 0.95, 0.72),
                    glass_panel as fn(&Theme) -> _,
                )
            } else {
                (
                    "🧭",
                    Color::from_rgb(0.74, 0.80, 1.0),
                    glass_panel as fn(&Theme) -> _,
                )
            };

            let open_item_button: Element<'_, Message> = if let Some(origin_ref) = &event.origin_ref
            {
                row![
                    button("Open item")
                        .padding([4, 10])
                        .style(rounded_secondary_button)
                        .on_press(Message::TimelineOpenItem(origin_ref.clone())),
                    button("Open chat")
                        .padding([4, 10])
                        .style(rounded_secondary_button)
                        .on_press(Message::OpenChatAtEvent(
                            origin_ref.clone(),
                            event.timestamp
                        )),
                ]
                .spacing(6)
                .into()
            } else {
                Space::new().width(0).into()
            };

            col.push(
                container(
                    row![
                        text(icon).size(16).color(tone),
                        text(event.line.clone()).size(13),
                        Space::new().width(Length::Fill),
                        open_item_button,
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding([8, 10])
                .style(panel),
            )
        });

    let last_sync = if state.audit_last_refresh_ts > 0 {
        format_local_time(state.audit_last_refresh_ts)
    } else {
        "never".to_string()
    };

    let status_banner: Element<'_, Message> = if state.audit_error.is_empty() {
        container(text(state.audit_status.clone()).size(13))
            .padding([8, 10])
            .style(glass_accent_panel)
            .into()
    } else {
        container(
            text(state.audit_error.clone())
                .size(13)
                .color([0.98, 0.70, 0.70]),
        )
        .padding([8, 10])
        .style(glass_alert_panel)
        .into()
    };

    let filter_bar: Element<'_, Message> =
        if let Some(origin_ref) = state.audit_origin_filter.as_ref() {
            container(
                row![
                    text(format!("Filter: {origin_ref}")).size(12),
                    Space::new().width(Length::Fill),
                    button("Clear filter")
                        .padding([4, 10])
                        .style(rounded_secondary_button)
                        .on_press(Message::AuditClearFilter),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([8, 10])
            .style(glass_panel)
            .into()
        } else {
            container(text("Showing all events").size(12))
                .padding([8, 10])
                .style(glass_panel)
                .into()
        };

    let content = column![
        row![
            text("Audit trail").size(18),
            Space::new().width(Length::Fill),
            inbox_chip("Events", filtered_events.len()),
            text(last_sync).size(12),
            Space::new().width(Length::Fill),
            button("Refresh")
                .padding([8, 12])
                .style(rounded_primary_button)
                .on_press(Message::AuditRefreshRequested),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        status_banner,
        filter_bar,
        lines,
    ]
    .spacing(10);

    container(
        scrollable(container(content).padding([4, 14]))
            .height(Length::Fill)
            .width(Length::Fill),
    )
    .padding(10)
    .style(glass_panel)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn inbox_status_label(status: InboxStatus) -> &'static str {
    match status {
        InboxStatus::New => "new",
        InboxStatus::Acknowledged => "acknowledged",
        InboxStatus::InProgress => "in_progress",
        InboxStatus::Blocked => "blocked",
        InboxStatus::Done => "done",
        InboxStatus::Dismissed => "dismissed",
    }
}

#[allow(dead_code)]
fn inbox_status_color(status: InboxStatus) -> Color {
    match status {
        InboxStatus::New => Color::from_rgb(0.64, 0.76, 1.0),
        InboxStatus::Acknowledged => Color::from_rgb(0.72, 0.82, 1.0),
        InboxStatus::InProgress => Color::from_rgb(0.73, 0.85, 1.0),
        InboxStatus::Blocked => Color::from_rgb(0.98, 0.55, 0.58),
        InboxStatus::Done => Color::from_rgb(0.62, 0.95, 0.70),
        InboxStatus::Dismissed => Color::from_rgb(0.68, 0.72, 0.78),
    }
}

fn view_chat_tab(state: &ButterflyIcedApp) -> Element<'_, Message> {
    let anchored_matches = state
        .chat_origin_anchor
        .as_ref()
        .map(|anchor| {
            state
                .chat_messages
                .iter()
                .filter(|msg| msg.text.contains(anchor))
                .count()
        })
        .unwrap_or(0);

    let anchor_banner: Element<'_, Message> =
        if let Some(anchor) = state.chat_origin_anchor.as_ref() {
            let anchor_id_text = state
                .chat_anchor_message_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_string());
            container(
                row![
                    text(format!("Chat anchor: {anchor}")).size(12),
                    Space::new().width(Length::Fill),
                    text(format!("message-id: {anchor_id_text}")).size(12),
                    inbox_chip("matches", anchored_matches),
                    button("Clear")
                        .padding([4, 10])
                        .style(rounded_secondary_button)
                        .on_press(Message::ChatClearAnchor),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding([8, 10])
            .style(glass_accent_panel)
            .into()
        } else {
            Space::new().height(0).into()
        };

    let list = state
        .chat_messages
        .iter()
        .filter(|msg| msg.role != MessageRole::System)
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
                            .style(rounded_primary_button)
                            .on_press(Message::CopyToClipboard(msg.text.clone()))
                    ]
                    .align_y(iced::Alignment::Center),
                    markdown::view(msg.markdown_items.iter(), markdown_render_settings())
                        .map(Message::MarkdownLinkClicked)
                ]
                .spacing(6),
            )
            .padding(12)
            .style(if state.chat_anchor_message_id == Some(msg.id) {
                glass_alert_panel
            } else if state
                .chat_origin_anchor
                .as_ref()
                .map(|anchor| msg.text.contains(anchor))
                .unwrap_or(false)
            {
                glass_accent_panel
            } else {
                match msg.role {
                    MessageRole::User => glass_user_bubble,
                    MessageRole::Bot => glass_bot_bubble,
                    MessageRole::System => glass_panel,
                }
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
            .style(rounded_primary_button)
            .on_press_maybe((!state.busy).then_some(Message::SendPressed)),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    column![
        container(
            scrollable(container(list).padding([0, 14]).width(Length::Fill))
                .id(state.chat_scroll_id.clone())
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
        anchor_banner,
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
                            .style(rounded_primary_button)
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
                            .style(rounded_danger_button)
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
                            .style(rounded_danger_button)
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
                    .style(rounded_success_button)
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
            text("Proactive Agent→Human chat policy").size(14),
            row![
                text(if state.settings.proactive_chat_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                })
                .size(13),
                Space::new().width(Length::Fill),
                button(if state.settings.proactive_chat_enabled {
                    "Disable"
                } else {
                    "Enable"
                })
                .padding([6, 10])
                .style(rounded_secondary_button)
                .on_press(Message::ToggleProactiveChatEnabled),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text_input(
                "Proactive min interval seconds",
                &state.settings.proactive_chat_min_interval_seconds
            )
            .on_input(Message::ProactiveChatMinIntervalChanged)
            .padding(8),
            text_input(
                "Proactive severity (blocked_only|blocked_or_overdue)",
                &state.settings.proactive_chat_severity
            )
            .on_input(Message::ProactiveChatSeverityChanged)
            .padding(8),
            row![
                text_input(
                    "Quiet start HH:MM (optional)",
                    &state.settings.proactive_chat_quiet_start_hhmm
                )
                .on_input(Message::ProactiveChatQuietStartChanged)
                .padding(8)
                .width(Length::FillPortion(1)),
                text_input(
                    "Quiet end HH:MM (optional)",
                    &state.settings.proactive_chat_quiet_end_hhmm
                )
                .on_input(Message::ProactiveChatQuietEndChanged)
                .padding(8)
                .width(Length::FillPortion(1)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(6))
        .padding(10)
        .style(glass_panel),
        container(column![
            text("MCP Servers").size(16),
            mcp_rows,
            button("+ Add MCP Server")
                .padding([8, 12])
                .style(rounded_primary_button)
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
                .style(rounded_primary_button)
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
                        .style(rounded_primary_button)
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
                        .style(rounded_primary_button)
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
        row![
            button("Run security doctor")
                .padding([8, 12])
                .style(rounded_primary_button)
                .on_press(Message::RunDoctorPressed),
            button("Refresh reminder delivery")
                .padding([8, 12])
                .style(rounded_secondary_button)
                .on_press(Message::RefreshReminderDeliveryEvents),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
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
        text(""),
        text(state.reminder_delivery_status.clone()),
        if state.reminder_delivery_error.is_empty() {
            text("")
        } else {
            text(state.reminder_delivery_error.clone()).color([0.95, 0.45, 0.45])
        },
        container(
            state
                .reminder_delivery_events
                .iter()
                .fold(column!().spacing(6), |col, line| col
                    .push(text(line.clone())))
        )
        .padding(8)
        .style(glass_panel),
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
                .style(rounded_success_button)
                .on_press(Message::SaveSettingsPressed),
            button("Reload")
                .padding([8, 12])
                .style(rounded_primary_button)
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
                .style(rounded_success_button)
                .on_press(Message::SaveSettingsPressed),
            button("Reload")
                .padding([8, 12])
                .style(rounded_primary_button)
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

fn count_actionable_human_items(items: &[InboxItem]) -> usize {
    items
        .iter()
        .filter(|item| item.requires_human_action && item.status.is_actionable())
        .count()
}

fn sync_actionable_badge(state: &mut ButterflyIcedApp) {
    let actionable = count_actionable_human_items(&state.inbox_items);
    if state.last_badge_actionable_count == Some(actionable) {
        return;
    }

    if let Err(err) = set_macos_dock_badge(actionable) {
        state.push_activity(format!("dock badge update failed: {err}"));
    }
    state.last_badge_actionable_count = Some(actionable);
}

fn set_macos_dock_badge(count: usize) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let label = if count == 0 {
            "".to_string()
        } else {
            count.to_string()
        };
        let script = format!(
            "ObjC.import('AppKit'); $.NSApplication.sharedApplication.dockTile.badgeLabel = '{}';",
            label.replace('\'', "\\'")
        );
        let _status = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", &script])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = count;
        Ok(())
    }
}

fn parse_inbox_status(value: Option<&str>, default: InboxStatus) -> InboxStatus {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "new" => InboxStatus::New,
        "ack" | "acknowledged" => InboxStatus::Acknowledged,
        "in_progress" | "in progress" | "started" => InboxStatus::InProgress,
        "blocked" => InboxStatus::Blocked,
        "done" | "completed" | "complete" => InboxStatus::Done,
        "dismissed" | "archived" => InboxStatus::Dismissed,
        _ => default,
    }
}

fn parse_inbox_priority(value: Option<&str>, default: InboxPriority) -> InboxPriority {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "low" => InboxPriority::Low,
        "high" => InboxPriority::High,
        "urgent" | "critical" => InboxPriority::Urgent,
        "normal" | "medium" => InboxPriority::Normal,
        _ => default,
    }
}

fn priority_rank(priority: InboxPriority) -> i32 {
    match priority {
        InboxPriority::Urgent => 0,
        InboxPriority::High => 1,
        InboxPriority::Normal => 2,
        InboxPriority::Low => 3,
    }
}

async fn load_inbox_items(
    daemon_url: String,
    token: String,
    user_id: String,
) -> Result<Vec<InboxItem>, String> {
    let extract_plan_step_ref = |details: Option<&String>| -> Option<String> {
        static PLAN_STEP_REF_RE: OnceLock<Regex> = OnceLock::new();
        let text = details?.as_str();
        let re = PLAN_STEP_REF_RE
            .get_or_init(|| Regex::new(r"(?i)planstepref\s*:\s*(plan_step:\d+:\d+)").unwrap());
        re.captures(text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_ascii_lowercase())
    };

    let client = daemon_request_client();
    let url = format!(
        "{}/inbox?user_id={}&limit=250&include_done=true",
        daemon_url.trim_end_matches('/'),
        user_id
    );
    let mut request = client.get(url);
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }

    let response = request.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!("Inbox request failed: HTTP {status}: {body}"));
    }

    let parsed = response
        .json::<InboxApiResponse>()
        .await
        .map_err(|err| err.to_string())?;

    let mut items = parsed
        .items
        .into_iter()
        .map(|item| {
            let source_type = match item.source_type.as_str() {
                "reminder" => InboxSourceType::Reminder,
                "todo" => InboxSourceType::Todo,
                "task" => InboxSourceType::Task,
                _ => InboxSourceType::PlanStep,
            };
            let default_status = parse_inbox_status(Some(&item.status), InboxStatus::New);
            let status = default_status;
            let priority = parse_inbox_priority(Some(&item.priority), InboxPriority::Normal);

            InboxItem {
                id: item.id,
                source_type,
                owner: item.owner,
                title: item.title,
                details: item.details,
                status,
                priority,
                due_at: item.due_at,
                created_at: item.created_at,
                updated_at: item.updated_at,
                requires_human_action: item.requires_human_action,
                origin_ref: item.origin_ref,
                dependency_refs: item.dependency_refs,
                t_shirt_size: item.t_shirt_size,
                story_points: item.story_points,
                estimate_optimistic_minutes: item.estimate_optimistic_minutes,
                estimate_likely_minutes: item.estimate_likely_minutes,
                estimate_pessimistic_minutes: item.estimate_pessimistic_minutes,
            }
        })
        .collect::<Vec<_>>();

    let plan_step_refs_in_view = items
        .iter()
        .filter(|item| item.source_type == InboxSourceType::PlanStep)
        .map(|item| item.origin_ref.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    items.retain(|item| {
        !(item.source_type == InboxSourceType::Todo
            && extract_plan_step_ref(item.details.as_ref())
                .map(|origin_ref| plan_step_refs_in_view.contains(&origin_ref))
                .unwrap_or(false))
    });

    let mut seen_origin_refs = HashSet::new();
    items.retain(|item| seen_origin_refs.insert(item.origin_ref.clone()));

    items.sort_by(|a, b| {
        priority_rank(a.priority)
            .cmp(&priority_rank(b.priority))
            .then_with(|| {
                a.due_at
                    .unwrap_or(i64::MAX)
                    .cmp(&b.due_at.unwrap_or(i64::MAX))
            })
            .then_with(|| b.created_at.cmp(&a.created_at))
    });

    Ok(items)
}

async fn apply_inbox_action(
    daemon_url: String,
    token: String,
    user_id: String,
    item: InboxItem,
    action: InboxActionKind,
) -> Result<String, String> {
    let action_name = match action {
        InboxActionKind::Acknowledge => "acknowledge",
        InboxActionKind::Start => "start",
        InboxActionKind::Block => "block",
        InboxActionKind::Done => "done",
        InboxActionKind::Reopen => "reopen",
        InboxActionKind::Snooze => "snooze",
    };

    let client = daemon_request_client();
    let url = format!("{}/inbox/transition", daemon_url.trim_end_matches('/'));
    let mut request = client.post(url).json(&serde_json::json!({
        "user_id": user_id,
        "origin_ref": item.origin_ref,
        "action": action_name,
    }));
    if !token.trim().is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }

    let response = request.send().await.map_err(|err| err.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("Inbox action failed: HTTP {status}: {body}"));
    }

    Ok(format!("Inbox action applied: {}", action_name))
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

    if running_from_macos_app_bundle() {
        return DaemonHealth {
            daemon_url: normalized,
            healthy: false,
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

async fn fetch_reminder_delivery_events(
    daemon_url: String,
    token: String,
    user_id: String,
    limit: usize,
) -> Result<Vec<String>, String> {
    let client = daemon_request_client();
    let url = format!(
        "{}/reminders/delivery_events?user_id={}&limit={}",
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
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!(
            "Reminder delivery diagnostics failed: HTTP {status}: {body}"
        ));
    }

    let parsed = response
        .json::<ReminderDeliveryEventsApiResponse>()
        .await
        .map_err(|err| err.to_string())?;

    let lines = parsed
        .events
        .into_iter()
        .map(|event| {
            let status = event
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let ts = event
                .get("timestamp")
                .and_then(|v| v.as_i64())
                .map(format_local_time)
                .unwrap_or_else(|| "unknown-time".to_string());
            let title = event
                .get("payload")
                .and_then(|v| v.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            format!("[{ts}] {status} — {title}")
        })
        .collect::<Vec<_>>();

    Ok(lines)
}

async fn fetch_audit_events(
    daemon_url: String,
    token: String,
    user_id: String,
    limit: usize,
) -> Result<Vec<AuditEventRow>, String> {
    let client = daemon_request_client();
    let url = format!(
        "{}/audit/events?user_id={}&limit={}",
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
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".to_string());
        return Err(format!(
            "Audit events request failed: HTTP {status}: {body}"
        ));
    }

    let parsed = response
        .json::<AuditEventsApiResponse>()
        .await
        .map_err(|err| err.to_string())?;

    let lines = parsed
        .events
        .into_iter()
        .map(|event| {
            let ts = event.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
            let ts_label = if ts > 0 {
                format_local_time(ts)
            } else {
                "unknown-time".to_string()
            };
            let event_type = event
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("event")
                .to_string();
            let status = event
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let tool = event
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let user = event.get("user_id").and_then(|v| v.as_str()).unwrap_or("-");
            let actor = event
                .get("payload")
                .and_then(|v| v.get("actor"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let origin_ref = event
                .get("payload")
                .and_then(|v| v.get("origin_ref"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            AuditEventRow {
                timestamp: ts,
                event_type: event_type.clone(),
                status: status.clone(),
                actor,
                line: format!(
                    "[{ts_label}] {event_type} • {tool} • {status} • {user} • origin:{}",
                    origin_ref.clone().unwrap_or_else(|| "-".to_string())
                ),
                origin_ref,
            }
        })
        .collect::<Vec<_>>();

    Ok(lines)
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

async fn run_clear_user_data_request(
    daemon_url: String,
    token: String,
    user_id: String,
) -> Result<(), String> {
    let client = daemon_request_client();
    let url = format!("{}/clear_user_data", daemon_url.trim_end_matches('/'));
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

fn optimistic_inbox_transition(
    state: &mut ButterflyIcedApp,
    origin_ref: &str,
    action: InboxActionKind,
) {
    let Some(item) = state
        .inbox_items
        .iter_mut()
        .find(|item| item.origin_ref == origin_ref)
    else {
        return;
    };

    let Some(previous) = parse_inbox_status_state(item.status) else {
        return;
    };
    let action = match action {
        InboxActionKind::Acknowledge => crate::inbox_fsm::InboxAction::Acknowledge,
        InboxActionKind::Start => crate::inbox_fsm::InboxAction::Start,
        InboxActionKind::Block => crate::inbox_fsm::InboxAction::Block,
        InboxActionKind::Done => crate::inbox_fsm::InboxAction::Done,
        InboxActionKind::Reopen => crate::inbox_fsm::InboxAction::Reopen,
        InboxActionKind::Snooze => crate::inbox_fsm::InboxAction::Snooze,
    };
    let Some(next) = crate::inbox_fsm::transition(previous, action) else {
        return;
    };
    item.status = next;
    item.updated_at = now_unix_ts();
}

fn parse_inbox_status_state(status: InboxStatus) -> Option<crate::inbox_fsm::InboxState> {
    match status {
        InboxStatus::New => Some(crate::inbox_fsm::InboxState::New),
        InboxStatus::Acknowledged => Some(crate::inbox_fsm::InboxState::Acknowledged),
        InboxStatus::InProgress => Some(crate::inbox_fsm::InboxState::InProgress),
        InboxStatus::Blocked => Some(crate::inbox_fsm::InboxState::Blocked),
        InboxStatus::Done => Some(crate::inbox_fsm::InboxState::Done),
        InboxStatus::Dismissed => Some(crate::inbox_fsm::InboxState::Dismissed),
    }
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

fn find_latest_chat_message_id(
    messages: &[ChatMessage],
    origin_ref: &str,
    at_or_before_ts: Option<i64>,
) -> Option<u64> {
    let by_origin = messages
        .iter()
        .rev()
        .find(|msg| {
            msg.text.contains(origin_ref)
                && at_or_before_ts
                    .map(|ts| msg.timestamp <= ts)
                    .unwrap_or(true)
        })
        .map(|msg| msg.id);
    if by_origin.is_some() {
        return by_origin;
    }

    at_or_before_ts.and_then(|ts| {
        messages
            .iter()
            .rev()
            .find(|msg| msg.timestamp <= ts)
            .map(|msg| msg.id)
    })
}

fn maybe_emit_proactive_chat_nudge(state: &mut ButterflyIcedApp) {
    if !state.settings.proactive_chat_enabled {
        return;
    }

    if in_quiet_hours(
        &state.settings.proactive_chat_quiet_start_hhmm,
        &state.settings.proactive_chat_quiet_end_hhmm,
    ) {
        return;
    }

    let now = now_unix_ts();
    let min_interval_seconds = state
        .settings
        .proactive_chat_min_interval_seconds
        .trim()
        .parse::<i64>()
        .unwrap_or(45)
        .max(5);
    let severity = state
        .settings
        .proactive_chat_severity
        .trim()
        .to_ascii_lowercase();
    let blocked_only = severity == "blocked_only";

    let candidates = state
        .inbox_items
        .iter()
        .filter(|item| {
            item.requires_human_action
                && item.status.is_actionable()
                && if blocked_only {
                    item.status == InboxStatus::Blocked
                } else {
                    item.status == InboxStatus::Blocked
                        || item.due_at.map(|due| due <= now).unwrap_or(false)
                }
        })
        .collect::<Vec<_>>();

    let active_origin_refs = candidates
        .iter()
        .map(|item| item.origin_ref.clone())
        .collect::<HashSet<_>>();
    state
        .proactive_notified_origin_refs
        .retain(|origin_ref| active_origin_refs.contains(origin_ref));

    if now.saturating_sub(state.proactive_last_chat_ts) < min_interval_seconds {
        return;
    }

    let mut ordered = candidates;
    ordered.sort_by_key(|item| {
        (
            item.status != InboxStatus::Blocked,
            priority_rank(item.priority),
            item.due_at.unwrap_or(i64::MAX),
        )
    });

    let Some(item) = ordered
        .into_iter()
        .find(|item| {
            !state
                .proactive_notified_origin_refs
                .contains(&item.origin_ref)
        })
        .cloned()
    else {
        return;
    };

    let due_label = item
        .due_at
        .map(format_local_time)
        .unwrap_or_else(|| "no due date".to_string());
    let ask = if item.status == InboxStatus::Blocked {
        "I am blocked and need your decision to unblock this."
    } else {
        "This is overdue and needs your action now."
    };

    state.push_chat(
        MessageRole::Bot,
        format!(
            "Heads up: {} ({}) • due: {} • ref: {}. {} Next step: choose Acknowledge/Start/Done in Inbox.",
            item.title,
            inbox_status_label(item.status),
            due_label,
            item.origin_ref,
            ask
        ),
    );
    state
        .proactive_notified_origin_refs
        .insert(item.origin_ref.clone());
    state.proactive_last_chat_ts = now;
    state.push_activity(format!(
        "agent proactive chat nudge sent for {}",
        item.origin_ref
    ));
}

fn parse_hhmm_to_minutes(value: &str) -> Option<u16> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (h, m) = trimmed.split_once(':')?;
    let hour = h.trim().parse::<u16>().ok()?;
    let minute = m.trim().parse::<u16>().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some(hour * 60 + minute)
}

fn in_quiet_hours(start_hhmm: &str, end_hhmm: &str) -> bool {
    let Some(start) = parse_hhmm_to_minutes(start_hhmm) else {
        return false;
    };
    let Some(end) = parse_hhmm_to_minutes(end_hhmm) else {
        return false;
    };

    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let now_local = OffsetDateTime::now_utc().to_offset(local_offset);
    let current = now_local.hour() as u16 * 60 + now_local.minute() as u16;

    if start == end {
        return false;
    }
    if start < end {
        current >= start && current < end
    } else {
        current >= start || current < end
    }
}

fn scroll_chat_to_anchor_task(state: &ButterflyIcedApp) -> Task<Message> {
    let Some(anchor_id) = state.chat_anchor_message_id else {
        return Task::none();
    };

    let Some(index) = state
        .chat_messages
        .iter()
        .position(|msg| msg.id == anchor_id)
    else {
        return Task::none();
    };

    let denom = (state.chat_messages.len().saturating_sub(1)).max(1) as f32;
    let y = (index as f32 / denom).clamp(0.0, 1.0);

    iced::widget::operation::snap_to(
        state.chat_scroll_id.clone(),
        iced::widget::operation::RelativeOffset { x: 0.0, y },
    )
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

fn parse_yyyy_mm_dd_to_unix(value: &str) -> Option<i64> {
    let normalized = value.trim().replace('/', "-");
    let mut parts = normalized.split('-');
    let year = parts.next()?.trim().parse::<i32>().ok()?;
    let month = parts.next()?.trim().parse::<u8>().ok()?;
    let day = parts.next()?.trim().parse::<u8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    Some(
        PrimitiveDateTime::new(date, Time::MIDNIGHT)
            .assume_offset(local_offset)
            .unix_timestamp(),
    )
}

fn normalize_nlp_due_text(input: &str) -> String {
    static FROM_START_RE: OnceLock<Regex> = OnceLock::new();
    static FROM_NOW_RE: OnceLock<Regex> = OnceLock::new();

    let from_start_re = FROM_START_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d+)\s*(day|days|week|weeks|month|months)\s+from\s+start\b").unwrap()
    });
    let from_now_re = FROM_NOW_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d+)\s*(day|days|week|weeks|month|months)\s+from\s+now\b").unwrap()
    });

    let normalized = from_start_re.replace_all(input, "in $1 $2").to_string();
    from_now_re.replace_all(&normalized, "in $1 $2").to_string()
}

fn text_has_explicit_time(input: &str) -> bool {
    static TIME_RE: OnceLock<Regex> = OnceLock::new();
    let re = TIME_RE.get_or_init(|| {
        Regex::new(r"(?i)(\b\d{1,2}:\d{2}\b|\b\d{1,2}\s*(am|pm)\b|\bnoon\b|\bmidnight\b)").unwrap()
    });
    re.is_match(input)
}

fn local_midnight_unix(ts: i64) -> i64 {
    let Ok(utc_dt) = OffsetDateTime::from_unix_timestamp(ts) else {
        return ts;
    };
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_dt = utc_dt.to_offset(local_offset);
    let date = local_dt.date();
    PrimitiveDateTime::new(date, Time::MIDNIGHT)
        .assume_offset(local_offset)
        .unix_timestamp()
}

fn extract_explicit_year(input: &str) -> Option<i32> {
    static YEAR_RE: OnceLock<Regex> = OnceLock::new();
    let re = YEAR_RE.get_or_init(|| Regex::new(r"\b(20\d{2})\b").unwrap());
    re.captures(input)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok())
}

fn with_local_year(ts: i64, year: i32) -> Option<i64> {
    let utc_dt = OffsetDateTime::from_unix_timestamp(ts).ok()?;
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_dt = utc_dt.to_offset(local_offset);
    let month = local_dt.month();
    let mut day = local_dt.day();

    let date = loop {
        if let Ok(value) = Date::from_calendar_date(year, month, day) {
            break value;
        }
        if day == 1 {
            return None;
        }
        day -= 1;
    };

    let pdt = PrimitiveDateTime::new(
        date,
        Time::from_hms(local_dt.hour(), local_dt.minute(), local_dt.second()).ok()?,
    );
    Some(pdt.assume_offset(local_offset).unix_timestamp())
}

fn normalize_due_year_if_stale(ts: i64, input: &str) -> i64 {
    let now = now_unix_ts();
    let Some(explicit_year) = extract_explicit_year(input) else {
        return ts;
    };

    let Ok(now_local) = OffsetDateTime::from_unix_timestamp(now) else {
        return ts;
    };
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let current_year = now_local.to_offset(local_offset).year();

    if explicit_year >= current_year {
        return ts;
    }

    if let Some(mut adjusted) = with_local_year(ts, current_year) {
        if adjusted < now {
            if let Some(next_year) = with_local_year(ts, current_year + 1) {
                adjusted = next_year;
            }
        }
        return adjusted;
    }

    ts
}

fn infer_due_at_from_text(text: &str, anchor_ts: i64) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    static LABELED_RE: OnceLock<Regex> = OnceLock::new();
    static DATE_RE: OnceLock<Regex> = OnceLock::new();

    let labeled_re = LABELED_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:due(?:\s+date)?|deadline)\s*:\s*(\d{4}[-/]\d{2}[-/]\d{2})").unwrap()
    });
    if let Some(value) = labeled_re
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
    {
        if let Some(ts) = parse_yyyy_mm_dd_to_unix(value) {
            return Some(ts);
        }
    }

    let date_re = DATE_RE.get_or_init(|| Regex::new(r"(\d{4}[-/]\d{2}[-/]\d{2})").unwrap());
    if let Some(ts) = date_re
        .captures(trimmed)
        .and_then(|caps| caps.get(1))
        .and_then(|m| parse_yyyy_mm_dd_to_unix(m.as_str()))
    {
        return Some(normalize_due_year_if_stale(ts, trimmed));
    }

    let normalized = normalize_nlp_due_text(trimmed);
    let anchor = Local
        .timestamp_opt(anchor_ts, 0)
        .single()
        .or_else(|| Local.timestamp_opt(now_unix_ts(), 0).single())
        .unwrap_or_else(Local::now);

    parse_date_string(&normalized, anchor, Dialect::Us)
        .or_else(|_| parse_date_string(&normalized, anchor, Dialect::Uk))
        .ok()
        .map(|dt: DateTime<Local>| {
            let ts = dt.timestamp();
            let parsed = if text_has_explicit_time(trimmed) {
                ts
            } else {
                local_midnight_unix(ts)
            };
            normalize_due_year_if_stale(parsed, trimmed)
        })
}

fn infer_due_at_from_item_text(item: &InboxItem) -> Option<i64> {
    infer_due_at_from_text(&item.title, item.created_at).or_else(|| {
        item.details
            .as_deref()
            .and_then(|details| infer_due_at_from_text(details, item.created_at))
    })
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

fn format_due_badge_time(ts: i64) -> String {
    let Ok(utc_dt) = OffsetDateTime::from_unix_timestamp(ts) else {
        return "1970-01-01".to_string();
    };
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_dt = utc_dt.to_offset(local_offset);
    if local_dt.hour() == 0 && local_dt.minute() == 0 && local_dt.second() == 0 {
        format!(
            "{:04}-{:02}-{:02}",
            local_dt.year(),
            u8::from(local_dt.month()),
            local_dt.day()
        )
    } else {
        format_local_time(ts)
    }
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
                candidates.push(parent.join("Resources").join("butterfly-botd"));
            }
        }
    }
    candidates.push(PathBuf::from("butterfly-botd"));
    candidates
}

fn running_from_macos_app_bundle() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Ok(current) = std::env::current_exe() {
            let path = current.to_string_lossy();
            return path.contains(".app/Contents/MacOS/");
        }
    }
    false
}

async fn start_local_daemon(
    daemon_url: String,
    db_path: String,
    token: String,
) -> Result<String, String> {
    let (host, port) = parse_daemon_address(&daemon_url);
    let strict_bundle_mode = running_from_macos_app_bundle();
    let mut selected = None;
    for candidate in daemon_binary_candidates() {
        if strict_bundle_mode && candidate == Path::new("butterfly-botd") {
            continue;
        }
        if candidate.exists() || candidate == Path::new("butterfly-botd") {
            selected = Some(candidate);
            break;
        }
    }
    let Some(binary) = selected else {
        if strict_bundle_mode {
            return Err(
                "Could not find bundled butterfly-botd in app bundle. Rebuild the mac app with ./scripts/build-macos-app.sh."
                    .to_string(),
            );
        }
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
        let proactive_chat_enabled = get_path(tools, &["settings", "proactive_chat", "enabled"])
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let proactive_chat_min_interval_seconds = get_path(
            tools,
            &["settings", "proactive_chat", "min_interval_seconds"],
        )
        .and_then(|v| v.as_u64())
        .unwrap_or(45)
        .to_string();
        let proactive_chat_severity = get_path(tools, &["settings", "proactive_chat", "severity"])
            .and_then(|v| v.as_str())
            .unwrap_or("blocked_or_overdue")
            .to_string();
        let proactive_chat_quiet_start_hhmm =
            get_path(tools, &["settings", "proactive_chat", "quiet_start_hhmm"])
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
        let proactive_chat_quiet_end_hhmm =
            get_path(tools, &["settings", "proactive_chat", "quiet_end_hhmm"])
                .and_then(|v| v.as_str())
                .unwrap_or_default()
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
                proactive_chat_enabled,
                proactive_chat_min_interval_seconds,
                proactive_chat_severity,
                proactive_chat_quiet_start_hhmm,
                proactive_chat_quiet_end_hhmm,
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

                let proactive_chat = settings_obj
                    .entry("proactive_chat")
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                let proactive_chat_obj = proactive_chat
                    .as_object_mut()
                    .ok_or_else(|| "tools.settings.proactive_chat must be an object".to_string())?;
                let proactive_min_interval = form
                    .proactive_chat_min_interval_seconds
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(45)
                    .max(5);
                proactive_chat_obj.insert(
                    "enabled".to_string(),
                    Value::Bool(form.proactive_chat_enabled),
                );
                proactive_chat_obj.insert(
                    "min_interval_seconds".to_string(),
                    Value::Number(serde_json::Number::from(proactive_min_interval)),
                );
                proactive_chat_obj.insert(
                    "severity".to_string(),
                    Value::String(form.proactive_chat_severity.trim().to_ascii_lowercase()),
                );
                proactive_chat_obj.insert(
                    "quiet_start_hhmm".to_string(),
                    Value::String(form.proactive_chat_quiet_start_hhmm.trim().to_string()),
                );
                proactive_chat_obj.insert(
                    "quiet_end_hhmm".to_string(),
                    Value::String(form.proactive_chat_quiet_end_hhmm.trim().to_string()),
                );

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
