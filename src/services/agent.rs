use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_stream::try_stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::Serialize;
use serde_json::json;

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
            "INSTRUCTION: If a DUE REMINDERS section is present in the context, surface those reminders first. Respond to the CURRENT USER MESSAGE below. If the prompt context or heartbeat explicitly requires autonomous actions, you may take initiative by using tools to advance the task even without additional user prompts. When working on any multi-step objective, use the `planning` tool to create/update plans, the `todo` tool to track action items, and the `tasks` tool for scheduled work. Always explain your thinking before acting. If earlier history mentions self-harm but the current message does not, do not output crisis resources.\n\n",
        );
        full_prompt.push_str("CURRENT USER MESSAGE:\n");
        full_prompt.push_str(query);
        full_prompt.push_str(&format!("\n\nUSER IDENTIFIER: {}", user_id));

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

        let tool_names = tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        let tool_list = if tool_names.is_empty() {
            "none".to_string()
        } else {
            tool_names.join(", ")
        };
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
            "When using tools, call ONLY ONE tool per step. If no tool is needed, respond with the final answer.\n",
        );

        for tool in &tools {
            tool_specs.push(serde_json::json!({
                "type": "function",
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters(),
            }));
        }

        if !tools.is_empty() {
            prompt.push_str("\nTOOL SUMMARIES:\n");
            for tool in &tools {
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
                last_text = response.text.clone();
            }
            if response.tool_calls.is_empty() {
                return Ok(last_text);
            }

            let first_call = response
                .tool_calls
                .first()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>();

            let results = self
                .execute_tool_calls(&first_call, &tools, user_id)
                .await?;
            let serialized = serde_json::to_string_pretty(&results)
                .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
            prompt.push_str("\n\nOBSERVATION:\n");
            prompt.push_str(&serialized);
            prompt.push_str("\n\nContinue the ReAct loop. If done, provide the final response.\n");
        }

        Ok(last_text)
    }

    async fn execute_tool_calls(
        &self,
        calls: &[ToolCall],
        tools: &[Arc<dyn crate::interfaces::plugins::Tool>],
        user_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let mut results = Vec::new();
        for call in calls {
            let tool = tools.iter().find(|t| t.name() == call.name);
            match tool {
                Some(_tool) => {
                    let redacted_args = redact_value(&call.arguments);
                    info!(
                        tool = %call.name,
                        args = %serde_json::to_string(&redacted_args).unwrap_or_default(),
                        "Tool call"
                    );
                    let mut args = call.arguments.clone();
                    if let serde_json::Value::Object(ref mut map) = args {
                        if !map.contains_key("user_id") {
                            map.insert(
                                "user_id".to_string(),
                                serde_json::Value::String(user_id.to_string()),
                            );
                        }
                    }
                    match self.tool_registry.execute_tool(&call.name, args).await {
                        Ok(result) => {
                            let _ = self
                                .tool_registry
                                .audit_tool_call(&call.name, "success")
                                .await;
                            let result_clone = result.clone();
                            let redacted_result = redact_value(&result_clone);
                            info!(
                                tool = %call.name,
                                status = "success",
                                result = %serde_json::to_string(&redacted_result).unwrap_or_default(),
                                "Tool result"
                            );
                            self.emit_tool_event(
                                user_id,
                                &call.name,
                                "success",
                                serde_json::json!({ "args": redacted_args.clone(), "result": redacted_result }),
                            );
                            results.push(serde_json::json!({
                                "tool": call.name,
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
                                    || err_message.contains("WASM tool execute failed"));
                            if should_skip {
                                let _ = self
                                    .tool_registry
                                    .audit_tool_call(&call.name, "skipped")
                                    .await;
                                info!(
                                    tool = %call.name,
                                    status = "skipped",
                                    error = %redact_string(&err_message),
                                    "Tool result"
                                );
                                self.emit_tool_event(
                                    user_id,
                                    &call.name,
                                    "skipped",
                                    serde_json::json!({ "args": redacted_args.clone(), "error": redact_string(&err_message) }),
                                );
                                results.push(serde_json::json!({
                                    "tool": call.name,
                                    "status": "skipped",
                                    "error": err_message,
                                }));
                                continue;
                            }

                            let _ = self
                                .tool_registry
                                .audit_tool_call(&call.name, "error")
                                .await;
                            info!(
                                tool = %call.name,
                                status = "error",
                                error = %redact_string(&err_message),
                                "Tool result"
                            );
                            self.emit_tool_event(
                                user_id,
                                &call.name,
                                "error",
                                serde_json::json!({ "args": redacted_args.clone(), "error": redact_string(&err.to_string()) }),
                            );
                            return Err(err);
                        }
                    }
                }
                None => {
                    let redacted_args = redact_value(&call.arguments);
                    let _ = self
                        .tool_registry
                        .audit_tool_call(&call.name, "not_found")
                        .await;
                    info!(
                        tool = %call.name,
                        status = "not_found",
                        "Tool result"
                    );
                    self.emit_tool_event(
                        user_id,
                        &call.name,
                        "not_found",
                        serde_json::json!({ "args": redacted_args.clone(), "message": "Tool not found" }),
                    );
                    results.push(serde_json::json!({
                        "tool": call.name,
                        "status": "error",
                        "message": "Tool not found",
                    }));
                }
            }
        }
        Ok(results)
    }
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
