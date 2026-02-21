use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_stream::try_stream;
use base64::Engine as _;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};

use once_cell::sync::Lazy;
use regex::Regex;

use tracing::{debug, error, info, warn};

use crate::brain::manager::BrainManager;
use crate::domains::agent::AIAgent;
use crate::error::{ButterflyBotError, Result};
use crate::factories::agent_factory::load_markdown_source;
use crate::interfaces::brain::{BrainContext, BrainEvent};
use crate::interfaces::providers::{LlmProvider, ToolCall};
use crate::plugins::registry::ToolRegistry;
use crate::security::x402::{canonicalize_payment_required, CanonicalX402Intent};
use tokio::sync::broadcast;
use tokio::sync::RwLock;

pub struct AgentService {
    llm_provider: Arc<dyn LlmProvider>,
    pub tool_registry: Arc<ToolRegistry>,
    agent: AIAgent,
    context_source: Option<String>,
    context_markdown: RwLock<Option<String>>,
    heartbeat_markdown: RwLock<Option<String>>,
    prompt_markdown: RwLock<Option<String>>,
    last_context_refresh: RwLock<Option<Instant>>,
    context_refresh_guard: tokio::sync::Mutex<()>,
    brain_manager: Arc<BrainManager>,
    started: RwLock<bool>,
    ui_event_tx: Option<broadcast::Sender<UiEvent>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UiEvent {
    pub event_type: String,
    pub user_id: String,
    pub tool: String,
    pub status: String,
    pub payload: serde_json::Value,
    pub timestamp: i64,
}

impl AgentService {
    fn append_runtime_identity_context(&self, prompt: &mut String, user_id: &str) {
        let actor = "agent";
        match crate::security::solana_signer::wallet_address(user_id, actor) {
            Ok(address) => {
                prompt.push_str("\n\nRUNTIME IDENTITY:\n");
                prompt.push_str("- Agent actor: agent\n");
                prompt.push_str(&format!("- Solana wallet address: {address}\n"));
                prompt.push_str(
                    "- If the user asks for your Solana address, respond with this exact address.\n",
                );
            }
            Err(err) => {
                warn!(
                    "Failed to resolve runtime Solana wallet for user {}: {}",
                    user_id, err
                );
            }
        }
    }

    pub fn agent_name(&self) -> &str {
        &self.agent.name
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm_provider: Arc<dyn LlmProvider>,
        agent: AIAgent,
        context_source: Option<String>,
        context_markdown: Option<String>,
        heartbeat_markdown: Option<String>,
        prompt_markdown: Option<String>,
        brain_manager: Arc<BrainManager>,
        ui_event_tx: Option<broadcast::Sender<UiEvent>>,
    ) -> Self {
        Self {
            llm_provider,
            tool_registry: Arc::new(ToolRegistry::new()),
            agent,
            context_source,
            context_markdown: RwLock::new(context_markdown),
            heartbeat_markdown: RwLock::new(heartbeat_markdown),
            prompt_markdown: RwLock::new(prompt_markdown),
            last_context_refresh: RwLock::new(None),
            context_refresh_guard: tokio::sync::Mutex::new(()),
            brain_manager,
            started: RwLock::new(false),
            ui_event_tx,
        }
    }

    pub async fn set_heartbeat_markdown(&self, heartbeat_markdown: Option<String>) {
        let mut guard = self.heartbeat_markdown.write().await;
        *guard = heartbeat_markdown;
    }

    pub async fn set_prompt_markdown(&self, prompt_markdown: Option<String>) {
        let mut guard = self.prompt_markdown.write().await;
        *guard = prompt_markdown;
    }

    pub async fn refresh_context_for_user(&self, user_id: &str) -> Result<bool> {
        self.refresh_context(user_id).await
    }

    pub async fn get_context_markdown(&self) -> Option<String> {
        let guard = self.context_markdown.read().await;
        guard.clone()
    }

    async fn refresh_context(&self, user_id: &str) -> Result<bool> {
        // Debounce refresh to avoid stampede on boot.
        let refresh_interval = Duration::from_secs(30);
        if let Some(last) = *self.last_context_refresh.read().await {
            if last.elapsed() < refresh_interval {
                return Ok(true);
            }
        }

        let _guard = self.context_refresh_guard.lock().await;
        if let Some(last) = *self.last_context_refresh.read().await {
            if last.elapsed() < refresh_interval {
                return Ok(true);
            }
        }

        let Some(source) = &self.context_source else {
            let existing = self.context_markdown.read().await;
            if existing
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                return Ok(true);
            }
            warn!("No context_source URL configured and no context markdown in memory");
            return Ok(false);
        };
        if source.trim().is_empty() {
            warn!("context_source is blank – the agent has no context file");
            return Ok(false);
        }

        info!("Refreshing context from {}", source);
        match load_markdown_source(Some(source)).await {
            Ok(Some(text)) if !text.trim().is_empty() => {
                info!("Context loaded OK from {} ({} bytes)", source, text.len());
                let mut guard = self.context_markdown.write().await;
                *guard = Some(text.clone());
                let mut last_guard = self.last_context_refresh.write().await;
                *last_guard = Some(Instant::now());
                self.emit_tool_event(
                    user_id,
                    "context",
                    "ok",
                    json!({"source": source, "length": text.len()}),
                );
                Ok(true)
            }
            Ok(_) => {
                error!("Context source {} returned empty content", source);
                self.emit_tool_event(
                    user_id,
                    "context",
                    "error",
                    json!({"source": source, "error": "empty context markdown"}),
                );
                Ok(false)
            }
            Err(err) => {
                error!("Failed to load context from {}: {}", source, err);
                self.emit_tool_event(
                    user_id,
                    "context",
                    "error",
                    json!({"source": source, "error": err.to_string()}),
                );
                Err(ButterflyBotError::Config(format!(
                    "Failed to load context markdown from {}: {}",
                    source, err
                )))
            }
        }
    }

    fn emit_tool_event(&self, user_id: &str, tool: &str, status: &str, payload: serde_json::Value) {
        let Some(sender) = &self.ui_event_tx else {
            return;
        };
        let event = UiEvent {
            event_type: "tool".to_string(),
            user_id: user_id.to_string(),
            tool: tool.to_string(),
            status: status.to_string(),
            payload,
            timestamp: now_ts(),
        };
        let _ = sender.send(event);
    }

    async fn ensure_brain_started(&self, user_id: &str) -> Result<()> {
        let mut started = self.started.write().await;
        if !*started {
            *started = true;
            let ctx = BrainContext {
                agent_name: self.agent.name.clone(),
                user_id: Some(user_id.to_string()),
            };
            self.brain_manager.dispatch(BrainEvent::Start, &ctx).await;
        }
        Ok(())
    }

    pub async fn dispatch_brain_tick(&self) {
        let ctx = BrainContext {
            agent_name: self.agent.name.clone(),
            user_id: None,
        };
        self.brain_manager.dispatch(BrainEvent::Tick, &ctx).await;
    }

    pub async fn get_agent_system_prompt(&self) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?
            .as_secs();

        // NOTE: refresh_context() is called by callers BEFORE this method.
        // Do NOT re-fetch here – it swallows errors and causes silent failures.

        let context_guard = self.context_markdown.read().await;
        let has_context = context_guard.as_ref().is_some_and(|s| !s.trim().is_empty());
        let instructions = &self.agent.instructions;

        if has_context {
            debug!("Primary context markdown loaded; system prompt will reference memory only");
        } else {
            warn!("No primary context markdown loaded; system prompt will use defaults only");
        }

        let instructions_block = if has_context {
            format!(
                "{}\n\nCONTEXT NOTE: The full primary prompt context is stored in memory as CONTEXT_DOC. Use it as authoritative guidance.",
                instructions
            )
        } else {
            instructions.to_string()
        };

        let mut system_prompt = format!(
            "You are {}, an AI assistant with the following instructions:\n\n{}\n\nCurrent time (unix seconds): {}",
            self.agent.name, instructions_block, now
        );

        let heartbeat_guard = self.heartbeat_markdown.read().await;
        if let Some(heartbeat) = &*heartbeat_guard {
            if !heartbeat.trim().is_empty() {
                system_prompt.push_str("\n\nHEARTBEAT (markdown):\n");
                system_prompt.push_str(heartbeat);
            }
        }

        let prompt_guard = self.prompt_markdown.read().await;
        if let Some(prompt) = &*prompt_guard {
            if !prompt.trim().is_empty() {
                system_prompt.push_str("\n\nCUSTOM PROMPT (markdown):\n");
                system_prompt.push_str(prompt);
            }
        }

        system_prompt.push_str(
            "\n\nCONTEXT GOVERNANCE:\n- The primary prompt context markdown defines your identity and behavior and is authoritative.\n- Prioritize direct user conversation first: answer greetings, questions, and clarifications naturally and briefly.\n- Be autonomous when the context asks for it, but do not let autonomous checks interrupt or replace a direct reply to the latest user message.\n- Use tools proactively to advance user goals without asking for confirmation unless required by tool policy or missing information.\n- Use planning/todo/tasks to track work when there is an actual goal or actionable work item.\n- Before creating new plans or todos, always LIST existing ones first to avoid duplicates.\n",
        );
        system_prompt.push_str(
            "\n\nRESPONSE STYLE:\n- For normal user conversation, do NOT expose chain-of-thought or forced sections. Respond naturally.\n- Use explicit 'Thought/Plan/Action/Observation/Summary' formatting ONLY during explicit autonomy ticks or when a tool-run status report is requested.\n- Keep outputs concise and user-friendly.\n",
        );
        system_prompt.push_str(
            "- Do not say 'please wait', 'I will now', or announce future actions when tools are available. Execute required tool calls immediately in the same turn, then report completed results.\n",
        );
        system_prompt.push_str(
            "- Respond in the same language as the latest user message unless the user asked for translation.\n- Do not fabricate tool-call transcripts, parameter blocks, or tool outputs. Only describe tool actions that actually executed in this turn.\n",
        );

        let tool_names = self
            .tool_registry
            .get_agent_tools(&self.agent.name)
            .await
            .iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        let tool_list = if tool_names.is_empty() {
            "none".to_string()
        } else {
            tool_names.join(", ")
        };

        system_prompt.push_str(
            "\n\nTOOL POLICY:\n- When the user asks to set, list, snooze, complete, or delete reminders, you MUST call the reminders tool.\n- Do not claim a reminder was created/updated unless the tool call succeeds.\n- If reminder details are missing, ask a clarification instead of guessing.\n- When working on goals, projects, or multi-step objectives, you MUST use the `planning` tool to create and track plans.\n- Use the `todo` tool for manual, checklist-style action items the user completes explicitly.\n- Use the `tasks` tool only for time-based/scheduled or recurring actions (run_at/interval).\n- When implementing code, writing functions, or building features, you MUST use the `coding` tool - it uses a specialized coding model optimized for development work.\n- ALWAYS list existing plans/todos/tasks BEFORE creating new ones to avoid duplicates.\n- After completing an action, mark the corresponding todo as complete.\n",
        );
        system_prompt.push_str(
            "- For explicit Solana transfer requests, call the `solana` tool with action `transfer` directly.\n- For SOL-denominated amounts, pass the exact user amount using `amount_sol` (or `amount` as a decimal/string with SOL units) instead of manually converting to lamports.\n- 1 SOL = 1,000,000,000 lamports.\n- Do not ask for an extra confirmation step unless the user explicitly requested a dry-run/simulation-only flow.\n- Do not surface internal simulation/preflight details in the final user response unless the user asks for those details.\n",
        );
        system_prompt.push_str(
            "- The `solana` tool supports both native SOL transfers and SPL-token transfers when `mint` + `amount_atomic` are provided.\n",
        );
        system_prompt.push_str(
            "- For x402 payment URLs/challenges, do NOT send SOL to a URL, hostname, endpoint path, or arbitrary slug.\n- In x402 flows, first retrieve/inspect the payment requirement (typically via `http_call`) and only use a destination from `payTo` when it is a valid Solana public key.\n",
        );
        system_prompt.push_str(
            "- Do NOT rely on memory or prior responses to decide whether a tool is usable or whether credentials exist. Tool availability and credentials can change at any time.\n- When a tool is relevant to the current request, attempt the tool call and let the tool response determine success/failure.\n",
        );
        system_prompt.push_str(&format!(
            "\nAVAILABLE TOOLS (use ONLY these exact names): {tool_list}\n"
        ));

        Ok(system_prompt)
    }

    pub async fn generate_response(
        &self,
        user_id: &str,
        query: &str,
        memory_context: &str,
        prompt_override: Option<&str>,
    ) -> Result<String> {
        self.ensure_brain_started(user_id).await?;
        let ctx = BrainContext {
            agent_name: self.agent.name.clone(),
            user_id: Some(user_id.to_string()),
        };
        self.brain_manager
            .dispatch(
                BrainEvent::UserMessage {
                    user_id: user_id.to_string(),
                    text: query.to_string(),
                },
                &ctx,
            )
            .await;

        let processed_output = self
            .generate_response_inner(user_id, query, memory_context, prompt_override)
            .await?;

        self.brain_manager
            .dispatch(
                BrainEvent::AssistantResponse {
                    user_id: user_id.to_string(),
                    text: processed_output.clone(),
                },
                &ctx,
            )
            .await;

        Ok(processed_output)
    }

    async fn generate_response_inner(
        &self,
        user_id: &str,
        query: &str,
        memory_context: &str,
        prompt_override: Option<&str>,
    ) -> Result<String> {
        let context_loaded = self.refresh_context(user_id).await?;
        if !context_loaded {
            warn!("Proceeding without context file for user {}", user_id);
        }
        let system_prompt = self.get_agent_system_prompt().await?;
        let mut full_prompt = String::new();
        if !memory_context.is_empty() {
            full_prompt.push_str(
                "PAST CONVERSATION HISTORY (for reference only; do not respond to past messages; assistant statements are not facts about the user):\n",
            );
            full_prompt.push_str(memory_context);
            full_prompt.push_str("\n\n");
        }
        if let Some(prompt) = prompt_override {
            full_prompt.push_str("ADDITIONAL PROMPT:\n");
            full_prompt.push_str(prompt);
            full_prompt.push_str("\n\n");
        }
        full_prompt.push_str(
            "INSTRUCTION: If a DUE REMINDERS section is present in the context, surface those reminders first. Respond to the CURRENT USER MESSAGE below. If the prompt context or heartbeat explicitly requires autonomous actions, you may take initiative by using tools to advance the task even without additional user prompts. When working on any multi-step objective, use the `planning` tool to create/update plans, the `todo` tool to track action items, and the `tasks` tool for scheduled work. If earlier history mentions self-harm but the current message does not, do not output crisis resources.\n\n",
        );
        full_prompt.push_str("CURRENT USER MESSAGE:\n");
        full_prompt.push_str(query);
        full_prompt.push_str(&format!("\n\nUSER IDENTIFIER: {}", user_id));
        self.append_runtime_identity_context(&mut full_prompt, user_id);

        let tools = self.tool_registry.get_agent_tools(&self.agent.name).await;
        let output = if tools.is_empty() {
            self.llm_provider
                .generate_text(&full_prompt, &system_prompt, None)
                .await?
        } else {
            self.run_tool_loop(&system_prompt, &full_prompt, tools, user_id)
                .await?
        };
        Ok(output)
    }

    pub fn generate_response_stream<'a>(
        &'a self,
        user_id: &'a str,
        query: &'a str,
        memory_context: &'a str,
        prompt_override: Option<&'a str>,
    ) -> BoxStream<'a, Result<String>> {
        Box::pin(try_stream! {
            self.ensure_brain_started(user_id).await?;
            let context_loaded = self.refresh_context(user_id).await?;
            if !context_loaded {
                warn!("Proceeding without context file for user {} (stream)", user_id);
            }
            let ctx = BrainContext {
                agent_name: self.agent.name.clone(),
                user_id: Some(user_id.to_string()),
            };
            self.brain_manager
                .dispatch(
                    BrainEvent::UserMessage {
                        user_id: user_id.to_string(),
                        text: query.to_string(),
                    },
                    &ctx,
                )
                .await;

            let system_prompt = self.get_agent_system_prompt().await?;
            let mut full_prompt = String::new();
            if !memory_context.is_empty() {
                full_prompt.push_str(
                    "PAST CONVERSATION HISTORY (for reference only; do not respond to past messages; assistant statements are not facts about the user):\n",
                );
                full_prompt.push_str(memory_context);
                full_prompt.push_str("\n\n");
            }
            if let Some(prompt) = prompt_override {
                full_prompt.push_str("ADDITIONAL PROMPT:\n");
                full_prompt.push_str(prompt);
                full_prompt.push_str("\n\n");
            }
            full_prompt.push_str(
                "INSTRUCTION: If a DUE REMINDERS section is present in the context, surface those reminders first. Then respond only to the CURRENT USER MESSAGE below. If earlier history mentions self-harm but the current message does not, do not output crisis resources.\n\n",
            );
            full_prompt.push_str("CURRENT USER MESSAGE:\n");
            full_prompt.push_str(query);
            full_prompt.push_str(&format!("\n\nUSER IDENTIFIER: {}", user_id));
            self.append_runtime_identity_context(&mut full_prompt, user_id);

            let mut response_text = String::new();
            let tools = self.tool_registry.get_agent_tools(&self.agent.name).await;
            if !tools.is_empty() {
                let output = self
                    .run_tool_loop(&system_prompt, &full_prompt, tools, user_id)
                    .await?;
                if !output.is_empty() {
                    response_text.push_str(&output);
                    yield output;
                }
            } else {
                let mut messages = Vec::new();
                if !system_prompt.is_empty() {
                    messages.push(json!({"role": "system", "content": system_prompt}));
                }
                messages.push(json!({"role": "user", "content": full_prompt}));

                let mut stream = self.llm_provider.chat_stream(messages, None);
                while let Some(event) = stream.next().await {
                    let event = event?;
                    if let Some(error) = event.error {
                        Err(ButterflyBotError::Runtime(error))?;
                    }
                    if let Some(delta) = event.delta {
                        if !delta.is_empty() {
                            response_text.push_str(&delta);
                            yield delta;
                        }
                    }
                }
            }

            if !response_text.is_empty() {
                self.brain_manager
                    .dispatch(
                        BrainEvent::AssistantResponse {
                            user_id: user_id.to_string(),
                            text: response_text,
                        },
                        &ctx,
                    )
                    .await;
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn generate_response_with_images(
        &self,
        user_id: &str,
        query: &str,
        images: Vec<crate::interfaces::providers::ImageInput>,
        memory_context: &str,
        prompt_override: Option<&str>,
        detail: &str,
    ) -> Result<String> {
        let system_prompt = self.get_agent_system_prompt().await?;
        let mut full_prompt = String::new();
        if !memory_context.is_empty() {
            full_prompt.push_str(
                "PAST CONVERSATION HISTORY (for reference only; do not respond to past messages; assistant statements are not facts about the user):\n",
            );
            full_prompt.push_str(memory_context);
            full_prompt.push_str("\n\n");
        }
        if let Some(prompt) = prompt_override {
            full_prompt.push_str("ADDITIONAL PROMPT:\n");
            full_prompt.push_str(prompt);
            full_prompt.push_str("\n\n");
        }
        full_prompt.push_str(
            "INSTRUCTION: If a DUE REMINDERS section is present in the context, surface those reminders first. Then respond only to the CURRENT USER MESSAGE below. If earlier history mentions self-harm but the current message does not, do not output crisis resources.\n\n",
        );
        full_prompt.push_str("CURRENT USER MESSAGE:\n");
        full_prompt.push_str(query);
        full_prompt.push_str(&format!("\n\nUSER IDENTIFIER: {}", user_id));
        self.append_runtime_identity_context(&mut full_prompt, user_id);

        let output = self
            .llm_provider
            .generate_text_with_images(&full_prompt, images, &system_prompt, detail, None)
            .await?;
        Ok(output)
    }

    pub async fn generate_structured_response(
        &self,
        user_id: &str,
        query: &str,
        memory_context: &str,
        prompt_override: Option<&str>,
        json_schema: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let system_prompt = self.get_agent_system_prompt().await?;
        let mut full_prompt = String::new();
        if !memory_context.is_empty() {
            full_prompt.push_str(
                "PAST CONVERSATION HISTORY (for reference only; do not respond to past messages; assistant statements are not facts about the user):\n",
            );
            full_prompt.push_str(memory_context);
            full_prompt.push_str("\n\n");
        }
        if let Some(prompt) = prompt_override {
            full_prompt.push_str("ADDITIONAL PROMPT:\n");
            full_prompt.push_str(prompt);
            full_prompt.push_str("\n\n");
        }
        full_prompt.push_str(
            "INSTRUCTION: If a DUE REMINDERS section is present in the context, surface those reminders first. Then respond only to the CURRENT USER MESSAGE below. If earlier history mentions self-harm but the current message does not, do not output crisis resources.\n\n",
        );
        full_prompt.push_str("CURRENT USER MESSAGE:\n");
        full_prompt.push_str(query);
        full_prompt.push_str(&format!("\n\nUSER IDENTIFIER: {}", user_id));
        self.append_runtime_identity_context(&mut full_prompt, user_id);

        self.llm_provider
            .parse_structured_output(&full_prompt, &system_prompt, json_schema, None)
            .await
    }

    pub async fn transcribe_audio(
        &self,
        audio_bytes: Vec<u8>,
        input_format: &str,
    ) -> Result<String> {
        self.llm_provider
            .transcribe_audio(audio_bytes, input_format)
            .await
    }

    pub async fn synthesize_audio(
        &self,
        text: &str,
        voice: &str,
        response_format: &str,
    ) -> Result<Vec<u8>> {
        self.llm_provider.tts(text, voice, response_format).await
    }

    async fn run_tool_loop(
        &self,
        system_prompt: &str,
        initial_prompt: &str,
        tools: Vec<Arc<dyn crate::interfaces::plugins::Tool>>,
        user_id: &str,
    ) -> Result<String> {
        let mut prompt = initial_prompt.to_string();
        let mut last_text = String::new();
        let mut tool_specs = Vec::new();

        let has_solana_tool = tools.iter().any(|tool| tool.name() == "solana");
        let has_http_call_tool = tools.iter().any(|tool| tool.name() == "http_call");
        let has_workflow_tool = tools
            .iter()
            .any(|tool| matches!(tool.name(), "todo" | "tasks" | "reminders" | "planning"));
        let user_section = initial_prompt
            .split("CURRENT USER MESSAGE:\n")
            .nth(1)
            .unwrap_or(initial_prompt);
        let user_message_only = user_section
            .split("\n\nUSER IDENTIFIER:")
            .next()
            .unwrap_or(user_section)
            .trim();
        let normalized_user_prompt = user_message_only.to_ascii_lowercase();
        let looks_like_solana_request = normalized_user_prompt.contains("solana")
            || normalized_user_prompt.contains("lamports")
            || (normalized_user_prompt.contains("wallet")
                && (normalized_user_prompt.contains("balance")
                    || normalized_user_prompt.contains("address")
                    || normalized_user_prompt.contains("transfer")
                    || normalized_user_prompt.contains("transaction")
                    || normalized_user_prompt.contains("tx")));
        let looks_like_x402_request = normalized_user_prompt.contains("x402")
            || normalized_user_prompt.contains("payment required")
            || normalized_user_prompt.contains("paid-content")
            || normalized_user_prompt.contains("402 ");
        let looks_like_workflow_request = normalized_user_prompt.contains("todo")
            || normalized_user_prompt.contains("to-do")
            || normalized_user_prompt.contains("checklist")
            || normalized_user_prompt.contains("task list")
            || normalized_user_prompt.contains("task")
            || normalized_user_prompt.contains("tasks")
            || normalized_user_prompt.contains("reminder")
            || normalized_user_prompt.contains("reminders")
            || normalized_user_prompt.contains("plan")
            || normalized_user_prompt.contains("planning");
        let requires_workflow_tool_action = looks_like_workflow_request
            && (normalized_user_prompt.contains("create")
                || normalized_user_prompt.contains("add")
                || normalized_user_prompt.contains("list")
                || normalized_user_prompt.contains("show")
                || normalized_user_prompt.contains("set")
                || normalized_user_prompt.contains("schedule")
                || normalized_user_prompt.contains("delete")
                || normalized_user_prompt.contains("remove")
                || normalized_user_prompt.contains("cancel")
                || normalized_user_prompt.contains("disable")
                || normalized_user_prompt.contains("enable")
                || normalized_user_prompt.contains("complete")
                || normalized_user_prompt.contains("reopen")
                || normalized_user_prompt.contains("snooze")
                || normalized_user_prompt.contains("reorder")
                || normalized_user_prompt.contains("clear")
                || normalized_user_prompt.contains("update"));
        let requires_live_solana_data = normalized_user_prompt.contains("balance")
            || normalized_user_prompt.contains("wallet address")
            || normalized_user_prompt.contains("my address")
            || normalized_user_prompt.contains("transfer")
            || normalized_user_prompt.contains("send")
            || normalized_user_prompt.contains("transaction")
            || normalized_user_prompt.contains("tx ")
            || normalized_user_prompt.contains(" tx")
            || normalized_user_prompt.contains("history")
            || looks_like_x402_request;

        let active_tools: Vec<Arc<dyn crate::interfaces::plugins::Tool>> =
            if looks_like_x402_request && (has_solana_tool || has_http_call_tool) {
                tools
                    .iter()
                    .filter(|tool| tool.name() == "solana" || tool.name() == "http_call")
                    .cloned()
                    .collect()
            } else if has_solana_tool && looks_like_solana_request {
                tools
                    .iter()
                    .filter(|tool| tool.name() == "solana")
                    .cloned()
                    .collect()
            } else {
                tools.clone()
            };

        let tool_names = active_tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        let active_has_chain_tool = active_tools
            .iter()
            .any(|tool| tool.name() == "solana" || tool.name() == "http_call");
        let tool_list = if tool_names.is_empty() {
            "none".to_string()
        } else {
            tool_names.join(", ")
        };

        let mut solana_grounding_retries = 0usize;
        let mut workflow_grounding_retries = 0usize;
        let mut x402_transfer_retries = 0usize;
        let mut x402_solana_attempted = false;
        let mut has_executed_tool_call = false;
        let mut x402_intent: Option<CanonicalX402Intent> = None;
        let x402_strict_guard = looks_like_x402_request && has_http_call_tool;

        if requires_workflow_tool_action && !has_workflow_tool {
            return Ok("no-op: I can't execute this workflow request because `todo`/`tasks`/`reminders`/`planning` tools are not available for this agent.".to_string());
        }

        if looks_like_x402_request && has_http_call_tool {
            if let Some(url) = extract_first_http_url(user_message_only) {
                let preflight = vec![ToolCall {
                    name: "http_call".to_string(),
                    arguments: json!({
                        "method": "GET",
                        "url": url,
                    }),
                }];

                let preflight_results = self
                    .execute_tool_calls(
                        &preflight,
                        &active_tools,
                        user_id,
                        &mut x402_intent,
                        x402_strict_guard,
                    )
                    .await?;
                if !preflight_results.is_empty() {
                    has_executed_tool_call = true;
                    let serialized = serde_json::to_string_pretty(&preflight_results)
                        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
                    prompt.push_str("\n\nOBSERVATION (x402 preflight):\n");
                    prompt.push_str(&serialized);
                    prompt.push_str(
                        "\n\nUse the canonical x402 payment requirement above when constructing any payment transfer call.\n",
                    );
                }
            }
        }

        prompt.push_str(&format!(
            "\n\nAVAILABLE TOOLS (use ONLY these exact names): {tool_list}\n"
        ));
        prompt.push_str("Use tools as needed, then provide a clean final user-facing response.\n");
        prompt.push_str(
            "If you need a tool not listed, respond with 'no-op' and explain what is missing.\n",
        );
        prompt
            .push_str("Do NOT include Reason/Action/Observation sections in the final response.\n");
        prompt.push_str(
            "When using tools, you may call one or multiple tools in a step if needed. If no tool is needed, respond with the final answer.\n",
        );
        prompt.push_str(
            "Do not emit pre-action text like 'I'll do this now' or 'please wait'. Execute the tool call first, then provide results.\n",
        );

        if has_solana_tool {
            prompt.push_str(
                "If the user asks about Solana wallet address, balance, transfer, transaction status, or history, you MUST use the `solana` tool and MUST NOT use `http_call` or `mcp` for those operations.\n",
            );
            prompt.push_str(
                "For x402 payment flows, you may need both `http_call` (to inspect the payment requirement) and `solana` (to execute payment). Never pass a URL/slug as `to`; only a valid Solana public key belongs in `to`.\n",
            );
            prompt.push_str(
                "For x402 flows, tool names must be EXACTLY `http_call` or `solana` (or exact listed aliases only). Never embed JSON, markdown, or extra text in a tool name. Put all parameters in arguments only.\n",
            );
            prompt.push_str(
                "For `http_call` in x402, use a full `url` with `method`, and avoid `endpoint`/`server` shortcut fields unless they are explicitly configured.\n",
            );
        }

        for tool in &active_tools {
            tool_specs.push(serde_json::json!({
                "type": "function",
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters(),
            }));

            if tool.name() == "solana" {
                let alias_names = [
                    "solana_get_balance",
                    "solana_get_wallet",
                    "solana_transfer",
                    "solana_simulate_transfer",
                    "solana_tx_status",
                    "solana_tx_history",
                ];
                for alias in alias_names {
                    tool_specs.push(serde_json::json!({
                        "type": "function",
                        "name": alias,
                        "description": "Compatibility alias for `solana` tool. Prefer `solana` with an `action` field.",
                        "parameters": tool.parameters(),
                    }));
                }
            }
        }

        if !active_tools.is_empty() {
            prompt.push_str("\nTOOL SUMMARIES:\n");
            for tool in &active_tools {
                let desc = tool.description().trim();
                if desc.is_empty() {
                    prompt.push_str(&format!("- {}\n", tool.name()));
                } else {
                    prompt.push_str(&format!("- {}: {}\n", tool.name(), desc));
                }
            }
        }

        for _ in 0..20 {
            let response = self
                .llm_provider
                .generate_with_tools(&prompt, system_prompt, tool_specs.clone())
                .await?;
            if response.tool_calls.is_empty() && !response.text.is_empty() {
                if active_has_chain_tool
                    && !has_executed_tool_call
                    && requires_live_solana_data
                    && (looks_like_solana_request || looks_like_x402_request)
                {
                    if solana_grounding_retries < 3 {
                        solana_grounding_retries += 1;
                        prompt.push_str("\n\nSYSTEM CORRECTION:\nFor this request, you MUST call the `solana` tool before answering. If this is an x402 flow, call `http_call` first to inspect the payment requirement and then `solana` as needed. Do not guess wallet balances, addresses, lamports, or payment details.\n");
                        continue;
                    }

                    return Ok("I couldn't verify this with the Solana tools right now, so I won't guess. Please retry and I will fetch it via tool calls.".to_string());
                }

                if has_workflow_tool
                    && !has_executed_tool_call
                    && requires_workflow_tool_action
                    && !response.text.trim().is_empty()
                {
                    if workflow_grounding_retries < 3 {
                        workflow_grounding_retries += 1;
                        prompt.push_str("\n\nSYSTEM CORRECTION:\nFor this workflow request, you MUST call the relevant tool (`todo`, `tasks`, `reminders`, or `planning`) before answering. List existing items first when applicable to avoid duplicates, then create/update only as needed, and return completed outcomes (not planned actions).\n");
                        continue;
                    }

                    return Ok("I couldn't complete this workflow request with tools right now. Please retry and I will execute the required tool calls directly.".to_string());
                }

                let lowered = response.text.to_ascii_lowercase();
                let lowered_norm = lowered.replace('’', "'");
                let is_deferred_preaction = lowered.contains("please wait")
                    || lowered.contains("i will now")
                    || lowered.contains("i'll now")
                    || lowered.contains("i will first")
                    || lowered.contains("i'll first")
                    || lowered.contains("to proceed, i will")
                    || lowered.contains("i will:")
                    || lowered.contains("confirm if you'd like me to proceed")
                    || lowered.contains("confirm if you’d like me to proceed")
                    || lowered.contains("would you like me to proceed")
                    || lowered.contains("will execute")
                    || lowered.contains("let me check")
                    || lowered.contains("let me verify")
                    || lowered.contains("let me fetch")
                    || lowered.contains("executing:")
                    || lowered.contains("this will happen with")
                    || lowered.contains("i'll query")
                    || lowered.contains("i will query")
                    || lowered.contains("i'll verify")
                    || lowered.contains("i will verify")
                    || lowered.contains("i'll create it now")
                    || lowered.contains("i will create it now")
                    || lowered.contains("i'll schedule it now")
                    || lowered.contains("i will schedule it now")
                    || lowered.contains("i'll add it now")
                    || lowered.contains("i will add it now")
                    || lowered_norm.contains("i'll proceed")
                    || lowered_norm.contains("first, calling")
                    || lowered_norm.contains("first calling")
                    || lowered_norm.contains("calling to inspect")
                    || lowered_norm.contains("calling the ")
                    || lowered.contains("awaiting ")
                    || lowered.contains("hold -")
                    || lowered.contains("hold –")
                    || lowered.contains("tools activated")
                    || lowered.contains("listing existing")
                    || lowered.contains("to avoid duplicates")
                    || lowered.contains("creating a new one")
                    || lowered.contains("here's your clean todo list")
                    || lowered.contains("then creating")
                    || lowered.contains("then scheduling")
                    || lowered.contains("1/2")
                    || lowered.contains("2/2")
                    || lowered.contains("3… 2… 1")
                    || lowered.contains("3... 2... 1");
                let looks_like_fabricated_tool_report = lowered.contains("tool call")
                    || lowered.contains("tool output")
                    || lowered.contains("parameters:")
                    || lowered.contains("action:")
                    || lowered.contains("—(tool call)—")
                    || lowered.contains("```json")
                    || lowered.contains("[args]")
                    || lowered.contains("<special_")
                    || lowered.contains("solana[args]");
                let has_btc_units_in_solana_context = (looks_like_solana_request
                    || looks_like_x402_request)
                    && (lowered.contains("satoshi") || lowered.contains(" sat "));
                if is_deferred_preaction && !active_tools.is_empty() {
                    prompt.push_str("\n\nSYSTEM CORRECTION:\nDo not announce future actions. Execute the next required tool call now and return only completed outcomes.\n");
                    continue;
                }
                if looks_like_fabricated_tool_report && !active_tools.is_empty() {
                    prompt.push_str("\n\nSYSTEM CORRECTION:\nDo not output simulated/fabricated tool transcripts. Execute required tool calls and then provide a plain final response based only on actual tool results.\n");
                    continue;
                }
                if has_btc_units_in_solana_context {
                    prompt.push_str("\n\nSYSTEM CORRECTION:\nDo not use Bitcoin units (e.g., satoshi/sat) for Solana flows. Use SOL/lamports and tool-grounded values only.\n");
                    continue;
                }

                if looks_like_x402_request && has_solana_tool {
                    let claims_x402_not_supported = lowered
                        .contains("can't complete the x402 payment")
                        || lowered.contains("cannot complete the x402 payment")
                        || lowered.contains("only support sol transfers")
                        || lowered.contains("only supports sol transfers")
                        || lowered.contains("don't expose the x402")
                        || lowered.contains("do not expose the x402")
                        || lowered.contains("payment-signature")
                        || lowered.contains("payment-response header")
                        || lowered.contains("tools in this runtime only support sol");

                    let should_force_x402_solana = x402_intent.is_some() && !x402_solana_attempted;

                    if (should_force_x402_solana || claims_x402_not_supported)
                        && x402_transfer_retries < 3
                    {
                        x402_transfer_retries += 1;
                        prompt.push_str("\n\nSYSTEM CORRECTION:\nFor x402 requests, do NOT claim unsupported flow. You MUST attempt a `solana` tool call using `action=transfer` or `action=simulate_transfer` with canonical challenge fields (`to=payTo`, plus either `lamports` for SOL or `mint`+`amount_atomic` for SPL). Use actual tool output only.\n");
                        continue;
                    }
                }
                last_text = response.text.clone();
            }
            if response.tool_calls.is_empty() {
                return Ok(last_text);
            }

            has_executed_tool_call = true;

            let results = self
                .execute_tool_calls(
                    &response.tool_calls,
                    &active_tools,
                    user_id,
                    &mut x402_intent,
                    x402_strict_guard,
                )
                .await?;

            if results
                .iter()
                .any(|item| item.get("tool").and_then(|v| v.as_str()) == Some("solana"))
            {
                x402_solana_attempted = true;
            }

            if let Some(intent) = x402_intent.as_ref() {
                if let Some(summary) = grounded_x402_submission_response(intent, &results) {
                    return Ok(summary);
                }
            }
            if let Some(grounded) = grounded_solana_response(user_message_only, &results) {
                return Ok(grounded);
            }
            let serialized = serde_json::to_string_pretty(&results)
                .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
            prompt.push_str("\n\nOBSERVATION:\n");
            prompt.push_str(&serialized);
            prompt.push_str("\n\nContinue the ReAct loop. If done, provide the final response.\n");
        }

        if last_text.trim().is_empty() {
            if looks_like_x402_request {
                return Ok("I could not finish the x402 flow in this turn. I attempted tool execution but did not get a final grounded result yet. Please retry once and I will continue from the tool state.".to_string());
            }
            return Ok(
                "I could not produce a final response in this turn. Please retry.".to_string(),
            );
        }

        Ok(last_text)
    }

    async fn execute_tool_calls(
        &self,
        calls: &[ToolCall],
        tools: &[Arc<dyn crate::interfaces::plugins::Tool>],
        user_id: &str,
        x402_intent: &mut Option<CanonicalX402Intent>,
        x402_required: bool,
    ) -> Result<Vec<serde_json::Value>> {
        let mut results = Vec::new();
        for call in calls {
            let mut effective_name = normalize_tool_name(&call.name);
            let mut effective_args = call.arguments.clone();

            normalize_tool_arguments(&mut effective_args);

            if let Some(mapped_name) = map_tool_name_alias(&effective_name) {
                effective_name = mapped_name.to_string();
            }

            if let Some(action) = map_solana_alias_action(&effective_name) {
                effective_name = "solana".to_string();
                if let serde_json::Value::Object(ref mut map) = effective_args {
                    map.entry("action".to_string())
                        .or_insert_with(|| serde_json::Value::String(action.to_string()));
                }
            }

            if effective_name == "solana" {
                normalize_solana_action_and_aliases(&mut effective_args);
                harden_solana_transfer_args(
                    &mut effective_args,
                    x402_intent.as_ref(),
                    x402_required,
                )?;
            }

            let tool = tools.iter().find(|t| t.name() == effective_name);
            match tool {
                Some(_tool) => {
                    let redacted_args = redact_value(&effective_args);
                    info!(
                        tool = %effective_name,
                        args = %serde_json::to_string(&redacted_args).unwrap_or_default(),
                        "Tool call"
                    );
                    let mut args = effective_args.clone();
                    if let serde_json::Value::Object(ref mut map) = args {
                        if !map.contains_key("user_id") {
                            map.insert(
                                "user_id".to_string(),
                                serde_json::Value::String(user_id.to_string()),
                            );
                        }
                    }
                    match self.tool_registry.execute_tool(&effective_name, args).await {
                        Ok(result) => {
                            let invalid_args_payload = result
                                .get("status")
                                .and_then(|v| v.as_str())
                                .map(|v| v.eq_ignore_ascii_case("error"))
                                .unwrap_or(false)
                                && (result
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .map(|v| v.eq_ignore_ascii_case("invalid_args"))
                                    .unwrap_or(false)
                                    || result
                                        .get("error")
                                        .and_then(|v| v.as_str())
                                        .map(|v| {
                                            v.to_ascii_lowercase().contains("unsupported action")
                                        })
                                        .unwrap_or(false));

                            if invalid_args_payload {
                                let err_message = result
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("invalid args")
                                    .to_string();
                                let _ = self
                                    .tool_registry
                                    .audit_tool_call(&effective_name, "skipped")
                                    .await;
                                info!(
                                    tool = %effective_name,
                                    status = "skipped",
                                    error = %redact_string(&err_message),
                                    "Tool result"
                                );
                                self.emit_tool_event(
                                    user_id,
                                    &effective_name,
                                    "skipped",
                                    serde_json::json!({ "args": redacted_args.clone(), "error": redact_string(&err_message) }),
                                );
                                results.push(serde_json::json!({
                                    "tool": effective_name,
                                    "status": "skipped",
                                    "error": err_message,
                                }));
                                continue;
                            }

                            if effective_name == "http_call" {
                                maybe_capture_x402_intent_from_http_result(
                                    x402_intent,
                                    &result,
                                    user_id,
                                );
                            }

                            let _ = self
                                .tool_registry
                                .audit_tool_call(&effective_name, "success")
                                .await;
                            let result_clone = result.clone();
                            let redacted_result = redact_value(&result_clone);
                            info!(
                                tool = %effective_name,
                                status = "success",
                                result = %serde_json::to_string(&redacted_result).unwrap_or_default(),
                                "Tool result"
                            );
                            self.emit_tool_event(
                                user_id,
                                &effective_name,
                                "success",
                                serde_json::json!({ "args": redacted_args.clone(), "result": redacted_result }),
                            );
                            results.push(serde_json::json!({
                                "tool": effective_name,
                                "status": "success",
                                "result": result,
                            }));
                        }
                        Err(err) => {
                            let err_message = err.to_string();
                            let should_skip = matches!(err, ButterflyBotError::Runtime(_))
                                && (err_message.contains("No MCP servers configured")
                                    || err_message.contains("Unknown MCP server")
                                    || err_message.contains("Missing GitHub PAT")
                                    || err_message.contains("WASM module path does not exist")
                                    || err_message.contains("returned a stub response")
                                    || err_message.contains("WASM alloc failed")
                                    || err_message.contains("WASM tool input too large")
                                    || err_message.contains("WASM tool execute failed")
                                    || err_message.contains("builder error for url")
                                    || err_message.contains("relative URL without a base"));
                            if should_skip {
                                let _ = self
                                    .tool_registry
                                    .audit_tool_call(&effective_name, "skipped")
                                    .await;
                                info!(
                                    tool = %effective_name,
                                    status = "skipped",
                                    error = %redact_string(&err_message),
                                    "Tool result"
                                );
                                self.emit_tool_event(
                                    user_id,
                                    &effective_name,
                                    "skipped",
                                    serde_json::json!({ "args": redacted_args.clone(), "error": redact_string(&err_message) }),
                                );
                                results.push(serde_json::json!({
                                    "tool": effective_name,
                                    "status": "skipped",
                                    "error": err_message,
                                }));
                                continue;
                            }

                            let _ = self
                                .tool_registry
                                .audit_tool_call(&effective_name, "error")
                                .await;
                            info!(
                                tool = %effective_name,
                                status = "error",
                                error = %redact_string(&err_message),
                                "Tool result"
                            );
                            self.emit_tool_event(
                                user_id,
                                &effective_name,
                                "error",
                                serde_json::json!({ "args": redacted_args.clone(), "error": redact_string(&err.to_string()) }),
                            );
                            return Err(err);
                        }
                    }
                }
                None => {
                    let redacted_args = redact_value(&effective_args);
                    let _ = self
                        .tool_registry
                        .audit_tool_call(&effective_name, "not_found")
                        .await;
                    info!(
                        tool = %effective_name,
                        status = "not_found",
                        "Tool result"
                    );
                    self.emit_tool_event(
                        user_id,
                        &effective_name,
                        "not_found",
                        serde_json::json!({ "args": redacted_args.clone(), "message": "Tool not found" }),
                    );
                    results.push(serde_json::json!({
                        "tool": effective_name,
                        "status": "error",
                        "message": "Tool not found",
                    }));
                }
            }
        }
        Ok(results)
    }
}

fn map_solana_alias_action(name: &str) -> Option<&'static str> {
    let normalized = name.trim();
    match normalized {
        "solana.getBalance" | "solana.balance" | "solana_get_balance" => Some("balance"),
        "solana.getWallet" | "solana.wallet" | "solana.address" | "solana_get_wallet" => {
            Some("wallet")
        }
        "solana.transfer" | "solana.send" | "solana_transfer" => Some("transfer"),
        "solana.simulateTransfer" | "solana.simulate" | "solana_simulate_transfer" => {
            Some("simulate_transfer")
        }
        "solana.txStatus" | "solana.status" | "solana.signatureStatus" | "solana_tx_status" => {
            Some("tx_status")
        }
        "solana.txHistory" | "solana.history" | "solana_tx_history" => Some("tx_history"),
        _ => None,
    }
}

fn map_tool_name_alias(name: &str) -> Option<&'static str> {
    let normalized = name.trim();
    match normalized {
        "reminder" | "set_reminder" | "create_reminder" | "reminders.create" => Some("reminders"),
        _ => None,
    }
}

fn normalize_tool_name(name: &str) -> String {
    let mut candidate = name.trim();
    if let Some((prefix, _)) = candidate.split_once("\n") {
        candidate = prefix;
    }
    if let Some((prefix, _)) = candidate.split_once("[TOOL_CALLS]") {
        candidate = prefix;
    }

    candidate
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\''
                    | '`'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '「'
                    | '」'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | ':'
                    | ';'
                    | ','
            )
        })
        .to_string()
}

fn normalize_tool_arguments(args: &mut Value) {
    let Some(map) = args.as_object_mut() else {
        return;
    };

    let nested = map
        .get("parameters")
        .or_else(|| map.get("args"))
        .or_else(|| map.get("arguments"))
        .cloned();

    let Some(Value::Object(nested_map)) = nested else {
        return;
    };

    for (key, value) in nested_map {
        map.entry(key).or_insert(value);
    }

    map.remove("parameters");
    map.remove("args");
    map.remove("arguments");
}

fn extract_first_http_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let cleaned = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | ','
            )
        });
        if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
            Some(cleaned.to_string())
        } else {
            None
        }
    })
}

fn normalize_solana_action_and_aliases(args: &mut Value) {
    let Some(map) = args.as_object_mut() else {
        return;
    };

    if map.get("address").and_then(|v| v.as_str()).is_none() {
        if let Some(wallet_address) = map.get("wallet_address").cloned() {
            map.insert("address".to_string(), wallet_address);
        }
    }

    if let Some(action) = map.get("action").and_then(|v| v.as_str()) {
        let action_lc = action.trim().to_ascii_lowercase();
        let normalized = match action_lc.as_str() {
            "inspect_balance" | "check_balance" | "wallet_balance" => Some("balance"),
            "inspect_wallet" | "check_wallet" => Some("wallet"),
            "send_token" => Some("transfer"),
            "pay" | "payment" | "execute_payment" | "submit_payment" | "x402_payment" => {
                Some("transfer")
            }
            "simulate_payment" | "preview_payment" | "x402_preview" => Some("simulate_transfer"),
            "get_wallet_address" | "wallet_address" => Some("wallet"),
            "txstatus" | "check_tx" | "transaction_status" => Some("tx_status"),
            _ => None,
        };
        if let Some(next) = normalized {
            map.insert("action".to_string(), Value::String(next.to_string()));
            return;
        }

        let recognized = matches!(
            action_lc.as_str(),
            "wallet" | "balance" | "transfer" | "simulate_transfer" | "tx_status" | "tx_history"
        );
        if !recognized {
            let has_transfer_shape = map
                .get("to")
                .and_then(|v| v.as_str())
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
                || map.get("lamports").is_some()
                || map.get("amount_atomic").is_some()
                || map.get("mint").is_some();
            if has_transfer_shape {
                map.insert("action".to_string(), Value::String("transfer".to_string()));
            }
        }
    }
}

fn grounded_solana_response(user_message: &str, results: &[serde_json::Value]) -> Option<String> {
    let normalized = user_message.to_ascii_lowercase();
    for item in results.iter().rev() {
        if item.get("tool").and_then(|v| v.as_str()) != Some("solana") {
            continue;
        }
        if item.get("status").and_then(|v| v.as_str()) != Some("success") {
            continue;
        }

        let payload = item.get("result")?;
        let payload = payload
            .get("capability_result")
            .and_then(|value| value.get("result"))
            .unwrap_or(payload);

        if normalized.contains("balance") {
            let lamports = payload.get("lamports").and_then(|v| v.as_u64())?;
            let sol = payload
                .get("sol")
                .and_then(|v| v.as_f64())
                .unwrap_or(lamports as f64 / 1_000_000_000f64);
            return Some(format!(
                "Your Solana balance is {:.9} SOL ({} lamports).",
                sol, lamports
            ));
        }

        let asks_for_address = normalized.contains("wallet address")
            || normalized.contains("my address")
            || (normalized.contains("wallet") && normalized.contains("address"));
        if asks_for_address {
            if let Some(address) = payload
                .get("address")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                return Some(format!("Your Solana wallet address is {}.", address));
            }
        }
    }

    None
}

fn grounded_x402_submission_response(
    intent: &CanonicalX402Intent,
    results: &[serde_json::Value],
) -> Option<String> {
    for item in results.iter().rev() {
        if item.get("tool").and_then(|v| v.as_str()) != Some("solana") {
            continue;
        }
        if item.get("status").and_then(|v| v.as_str()) != Some("success") {
            continue;
        }

        let payload = item.get("result")?;
        let payload = payload
            .get("capability_result")
            .and_then(|value| value.get("result"))
            .unwrap_or(payload);

        if payload.get("status").and_then(|v| v.as_str()) != Some("submitted") {
            continue;
        }

        let signature = payload.get("signature").and_then(|v| v.as_str())?;
        let from_wallet = payload
            .get("wallet_address")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let amount = if x402_asset_is_native_sol(&intent.asset_id) {
            format!("{:.9} SOL", intent.amount_atomic as f64 / 1_000_000_000f64)
        } else {
            let payload_amount_atomic = payload
                .get("amount_atomic")
                .and_then(|v| v.as_u64())
                .unwrap_or(intent.amount_atomic);
            let decimals = payload
                .get("decimals")
                .and_then(|v| v.as_u64())
                .unwrap_or(6) as u32;
            let value = payload
                .get("ui_amount_string")
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format_atomic_with_decimals(payload_amount_atomic, decimals));

            let symbol = payload
                .get("mint")
                .and_then(|v| v.as_str())
                .map(asset_symbol)
                .unwrap_or_else(|| asset_symbol(&intent.asset_id));
            format!("{} {}", value, symbol)
        };

        return Some(format!(
            "x402 payment submitted successfully.\n- Amount: {}\n- To: {}\n- From wallet: {}\n- Signature: {}",
            amount, intent.payee, from_wallet, signature
        ));
    }

    None
}

fn format_atomic_with_decimals(amount: u64, decimals: u32) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let base = 10u128.saturating_pow(decimals);
    let whole = (amount as u128) / base;
    let frac = (amount as u128) % base;
    if frac == 0 {
        return whole.to_string();
    }
    let mut frac_str = format!("{:0width$}", frac, width = decimals as usize);
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{}.{}", whole, frac_str)
}

fn asset_symbol(asset_id: &str) -> String {
    let trimmed = asset_id.trim();
    if trimmed.eq_ignore_ascii_case("sol") {
        return "SOL".to_string();
    }
    if trimmed.eq_ignore_ascii_case("usdc")
        || trimmed.eq_ignore_ascii_case("usdcn")
        || trimmed.eq_ignore_ascii_case("usdⁿ")
        || trimmed == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    {
        return "USDC".to_string();
    }
    trimmed.to_string()
}

fn harden_solana_transfer_args(
    args: &mut serde_json::Value,
    x402_intent: Option<&CanonicalX402Intent>,
    x402_required: bool,
) -> Result<()> {
    let Some(map) = args.as_object_mut() else {
        return Ok(());
    };

    let action = map
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if action != "transfer" && action != "simulate_transfer" {
        return Ok(());
    }

    if x402_required && x402_intent.is_none() {
        return Err(ButterflyBotError::Runtime(
            "x402 payment requirement not resolved yet; refusing to run Solana transfer without canonical x402 intent"
                .to_string(),
        ));
    }

    let current_to = map
        .get("to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let current_to_valid = !current_to.is_empty() && is_valid_solana_pubkey(current_to);

    if let Some(intent) = x402_intent {
        if is_valid_solana_pubkey(&intent.payee) {
            map.insert("to".to_string(), Value::String(intent.payee.clone()));
        }

        if x402_asset_is_native_sol(&intent.asset_id) {
            map.insert("lamports".to_string(), Value::from(intent.amount_atomic));
            map.remove("amount_sol");
            map.remove("amount");
            map.remove("mint");
            map.remove("amount_atomic");
        }
    } else if !current_to_valid {
        // Non-x402 fallback: still require valid destination address when no canonical intent exists.
    }

    if let Some(intent) = x402_intent {
        if !x402_asset_is_native_sol(&intent.asset_id) {
            map.insert("mint".to_string(), Value::String(intent.asset_id.clone()));
            map.insert(
                "amount_atomic".to_string(),
                Value::from(intent.amount_atomic),
            );
            map.remove("lamports");
            map.remove("amount_sol");
            map.remove("amount");
        }
    }

    let final_to = map
        .get("to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    if final_to.is_empty() {
        return Err(ButterflyBotError::Runtime(
            "Missing `to` destination for Solana transfer".to_string(),
        ));
    }

    if !is_valid_solana_pubkey(final_to) {
        if looks_like_http_or_endpoint(final_to) {
            return Err(ButterflyBotError::Runtime(
                "Refusing Solana transfer: `to` must be a valid Solana public key, not a URL/endpoint"
                    .to_string(),
            ));
        }
        return Err(ButterflyBotError::Runtime(
            "Invalid `to` destination for Solana transfer".to_string(),
        ));
    }

    Ok(())
}

fn maybe_capture_x402_intent_from_http_result(
    state: &mut Option<CanonicalX402Intent>,
    http_result: &Value,
    user_id: &str,
) {
    let Some(challenge) = extract_x402_payment_required(http_result) else {
        return;
    };

    let request_id = format!("x402-{}", now_ts());
    let Ok((canonical, _)) =
        canonicalize_payment_required(&request_id, "agent", user_id, &challenge, None, false, None)
    else {
        if let Some(fallback) = fallback_canonical_x402_intent(&challenge) {
            *state = Some(fallback);
        }
        return;
    };

    if !is_valid_solana_pubkey(&canonical.payee) {
        return;
    }

    *state = Some(canonical);
}

fn extract_x402_payment_required(http_result: &Value) -> Option<Value> {
    let payload = http_result
        .get("capability_result")
        .and_then(|value| value.get("result"))
        .unwrap_or(http_result);

    if let Some(header_encoded) = payload
        .get("headers")
        .and_then(|headers| headers.get("payment-required"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(header_encoded) {
            if let Ok(value) = serde_json::from_slice::<Value>(&decoded) {
                if looks_like_x402_shape(&value) {
                    return Some(value);
                }
            }
        }
    }

    let json_payload = payload.get("json")?;

    if looks_like_x402_shape(json_payload) {
        return Some(json_payload.clone());
    }

    for key in ["payment_required", "paymentRequired"] {
        if let Some(candidate) = json_payload.get(key) {
            if looks_like_x402_shape(candidate) {
                return Some(candidate.clone());
            }
        }
    }

    None
}

fn fallback_canonical_x402_intent(challenge: &Value) -> Option<CanonicalX402Intent> {
    let accepts = challenge.get("accepts")?.as_array()?;
    let exact = accepts
        .iter()
        .find(|entry| entry.get("scheme").and_then(|v| v.as_str()) == Some("exact"))?;

    let chain_id = exact
        .get("network")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?
        .to_string();
    if !chain_id.starts_with("solana:") {
        return None;
    }

    let amount_atomic = exact
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|v| v.trim().parse::<u64>().ok())?;
    let payee = exact
        .get("payTo")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?
        .to_string();
    let asset_id = exact
        .get("asset")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("SOL")
        .to_string();
    let max_timeout = exact
        .get("maxTimeoutSeconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);
    let payment_authority = exact
        .get("extra")
        .and_then(|v| v.get("facilitator"))
        .or_else(|| exact.get("extra").and_then(|v| v.get("resource")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_authority")
        .to_string();

    Some(CanonicalX402Intent {
        scheme_id: "v2-solana-exact".to_string(),
        chain_id,
        asset_id,
        amount_atomic,
        payee,
        payment_authority,
        request_expiry: now_ts() as u64 + max_timeout,
        idempotency_key: format!("x402-{}", now_ts()),
        context_requires_approval: false,
    })
}

fn looks_like_x402_shape(value: &Value) -> bool {
    value.get("x402Version").and_then(|v| v.as_u64()).is_some()
        && value
            .get("accepts")
            .and_then(|v| v.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
}

fn looks_like_http_or_endpoint(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.starts_with("http://")
        || lowered.starts_with("https://")
        || lowered.contains('/')
        || lowered.contains(':')
}

fn is_valid_solana_pubkey(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Ok(bytes) = bs58::decode(trimmed).into_vec() else {
        return false;
    };
    bytes.len() == 32
}

fn x402_asset_is_native_sol(asset: &str) -> bool {
    let normalized = asset.trim().to_ascii_lowercase();
    normalized == "sol"
        || normalized == "native"
        || normalized.contains("lamport")
        || normalized.contains("solana")
}

static BEARER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bbearer\s+[^\s\x22\x27]+").unwrap());
static KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(sk-|xai-|github_pat_|ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9_\-]+").unwrap()
});

fn redact_string(input: &str) -> String {
    let mut out = BEARER_RE
        .replace_all(input, "Bearer [REDACTED]")
        .to_string();
    out = KEY_RE.replace_all(&out, "$1[REDACTED]").to_string();
    truncate_string(&out, 2000)
}

fn redact_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Null => serde_json::Value::Null,
        serde_json::Value::Bool(v) => serde_json::Value::Bool(*v),
        serde_json::Value::Number(v) => serde_json::Value::Number(v.clone()),
        serde_json::Value::String(v) => serde_json::Value::String(redact_string(v)),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(redact_value)
                .collect::<Vec<serde_json::Value>>(),
        ),
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if is_sensitive_key(key) {
                    out.insert(
                        key.clone(),
                        serde_json::Value::String("[REDACTED]".to_string()),
                    );
                } else {
                    out.insert(key.clone(), redact_value(value));
                }
            }
            serde_json::Value::Object(out)
        }
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("authorization")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("pat")
}

fn truncate_string(value: &str, _max_len: usize) -> String {
    value.to_string()
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
