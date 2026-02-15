use std::sync::Arc;

use async_stream::try_stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use std::hash::Hasher;
use std::time::Instant;

use md5::{Digest, Md5};

use crate::error::Result;
use crate::interfaces::providers::{ImageInput, MemoryProvider};
use crate::reminders::ReminderStore;
use crate::services::agent::AgentService;
use tracing::info;
use crate::vault;

#[derive(Debug, Clone)]
pub enum UserInput {
    Text(String),
    Audio {
        bytes: Vec<u8>,
        input_format: String,
    },
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Text,
    Audio { voice: String, format: String },
}

#[derive(Clone)]
pub struct ProcessOptions {
    pub prompt: Option<String>,
    pub images: Vec<ImageInput>,
    pub output_format: OutputFormat,
    pub image_detail: String,
    pub json_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum ProcessResult {
    Text(String),
    Audio(Vec<u8>),
    Structured(serde_json::Value),
}

pub struct QueryService {
    agent_service: Arc<AgentService>,
    memory_provider: Option<Arc<dyn MemoryProvider>>,
    reminder_store: Option<Arc<ReminderStore>>,
    context_cache: tokio::sync::RwLock<Option<u64>>,
}

impl QueryService {
    pub fn new(
        agent_service: Arc<AgentService>,
        memory_provider: Option<Arc<dyn MemoryProvider>>,
        reminder_store: Option<Arc<ReminderStore>>,
    ) -> Self {
        Self {
            agent_service,
            memory_provider,
            reminder_store,
            context_cache: tokio::sync::RwLock::new(None),
        }
    }

    async fn ensure_context_in_memory(&self, user_id: &str) -> Result<()> {
        let started = Instant::now();
        let Some(provider) = &self.memory_provider else {
            return Ok(());
        };
        let _ = self.agent_service.refresh_context_for_user(user_id).await?;
        info!("ensure_context_in_memory: refresh_context took {:?}", started.elapsed());
        let Some(context_markdown) = self.agent_service.get_context_markdown().await else {
            return Ok(());
        };
        if context_markdown.trim().is_empty() {
            return Ok(());
        }

        let mut md5_hasher = Md5::new();
        md5_hasher.update(context_markdown.as_bytes());
        let md5_hash = format!("{:x}", md5_hasher.finalize());
        if let Ok(Some(stored)) = vault::get_secret("context_md5") {
            if stored == md5_hash {
                let mut guard = self.context_cache.write().await;
                if guard.is_none() {
                    *guard = Some(0);
                }
                info!("ensure_context_in_memory: md5 unchanged, skipping import (elapsed {:?})", started.elapsed());
                return Ok(());
            }
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&context_markdown, &mut hasher);
        let hash = hasher.finish();

        let mut guard = self.context_cache.write().await;
        if guard.map_or(true, |prev| prev != hash) {
            let content = format!("CONTEXT_DOC:\n{}", context_markdown);
            provider.append_message(user_id, "context", &content).await?;
            *guard = Some(hash);
            let _ = vault::set_secret("context_md5", &md5_hash);
            info!("ensure_context_in_memory: imported context into memory (elapsed {:?})", started.elapsed());
        }
        Ok(())
    }

    pub async fn process_text(
        &self,
        user_id: &str,
        query: &str,
        prompt: Option<&str>,
    ) -> Result<String> {
        let processed_query = query.to_string();
        let autonomy_tick = is_autonomy_tick(&processed_query);

        self.ensure_context_in_memory(user_id).await?;

        if let Some(response) = self
            .try_handle_search_command(user_id, &processed_query)
            .await?
        {
            if let Some(provider) = &self.memory_provider {
                provider
                    .append_message(user_id, "user", &processed_query)
                    .await?;
                provider
                    .append_message(user_id, "assistant", &response)
                    .await?;
            }
            return Ok(response);
        }

        let reminder_context = if let Some(store) = &self.reminder_store {
            build_reminder_context(store, user_id).await
        } else {
            None
        };
        let mut memory_context = if let Some(provider) = &self.memory_provider {
            let include_semantic = should_include_semantic_memory(&processed_query);
            let history_future = provider.get_history(user_id, 12);
            let semantic_future = async {
                if include_semantic {
                    provider.search(user_id, &processed_query, 5).await
                } else {
                    Ok(Vec::new())
                }
            };
            let (history, semantic) = tokio::try_join!(history_future, semantic_future)?;
            let history = history.join("\n");
            build_memory_context(history, semantic, reminder_context)
        } else {
            reminder_context.unwrap_or_default()
        };

        if let Some(context_markdown) = self.context_for_autonomy(user_id, &processed_query).await {
            if !memory_context.is_empty() {
                memory_context.push_str("\n\n");
            }
            memory_context.push_str(&context_markdown);
        }

        let response = self
            .agent_service
            .generate_response(user_id, &processed_query, &memory_context, prompt)
            .await?;

        if let Some(provider) = &self.memory_provider {
            if autonomy_tick {
                return Ok(response);
            }
            provider
                .append_message(user_id, "user", &processed_query)
                .await?;
            provider
                .append_message(user_id, "assistant", &response)
                .await?;
        }

        Ok(response)
    }

    pub async fn process(
        &self,
        user_id: &str,
        input: UserInput,
        options: ProcessOptions,
    ) -> Result<ProcessResult> {
        let text = match input {
            UserInput::Text(value) => value,
            UserInput::Audio {
                bytes,
                input_format,
            } => {
                self.agent_service
                    .transcribe_audio(bytes, &input_format)
                    .await?
            }
        };
            let autonomy_tick = is_autonomy_tick(&text);

        self.ensure_context_in_memory(user_id).await?;

        if let Some(response) = self.try_handle_search_command(user_id, &text).await? {
            if let Some(provider) = &self.memory_provider {
                provider.append_message(user_id, "user", &text).await?;
                provider
                    .append_message(user_id, "assistant", &response)
                    .await?;
            }
            return Ok(ProcessResult::Text(response));
        }

        let reminder_context = if let Some(store) = &self.reminder_store {
            build_reminder_context(store, user_id).await
        } else {
            None
        };
        let mut memory_context = if let Some(provider) = &self.memory_provider {
            let include_semantic = should_include_semantic_memory(&text);
            let history_future = provider.get_history(user_id, 12);
            let semantic_future = async {
                if include_semantic {
                    provider.search(user_id, &text, 5).await
                } else {
                    Ok(Vec::new())
                }
            };
            let (history, semantic) = tokio::try_join!(history_future, semantic_future)?;
            let history = history.join("\n");
            build_memory_context(history, semantic, reminder_context)
        } else {
            reminder_context.unwrap_or_default()
        };

        if let Some(context_markdown) = self.context_for_autonomy(user_id, &text).await {
            if !memory_context.is_empty() {
                memory_context.push_str("\n\n");
            }
            memory_context.push_str(&context_markdown);
        }

        let result = if let Some(schema) = options.json_schema {
            let structured = self
                .agent_service
                .generate_structured_response(
                    user_id,
                    &text,
                    &memory_context,
                    options.prompt.as_deref(),
                    schema,
                )
                .await?;
            ProcessResult::Structured(structured)
        } else if !options.images.is_empty() {
            let response = self
                .agent_service
                .generate_response_with_images(
                    user_id,
                    &text,
                    options.images,
                    &memory_context,
                    options.prompt.as_deref(),
                    &options.image_detail,
                )
                .await?;
            ProcessResult::Text(response)
        } else {
            let response = self
                .agent_service
                .generate_response(user_id, &text, &memory_context, options.prompt.as_deref())
                .await?;
            ProcessResult::Text(response)
        };

        let output = match (result, options.output_format) {
            (ProcessResult::Text(text), OutputFormat::Audio { voice, format }) => {
                let bytes = self
                    .agent_service
                    .synthesize_audio(&text, &voice, &format)
                    .await?;
                ProcessResult::Audio(bytes)
            }
            (other, _) => other,
        };

        if let Some(provider) = &self.memory_provider {
            if autonomy_tick {
                return Ok(output);
            }
            provider.append_message(user_id, "user", &text).await?;
            if let ProcessResult::Text(ref message) = output {
                provider
                    .append_message(user_id, "assistant", message)
                    .await?;
            }
        }

        Ok(output)
    }

    pub fn process_text_stream<'a>(
        &'a self,
        user_id: &'a str,
        query: &'a str,
        prompt: Option<&'a str>,
    ) -> BoxStream<'a, Result<String>> {
        Box::pin(try_stream! {
            let processed_query = query.to_string();
            let autonomy_tick = is_autonomy_tick(&processed_query);

            self.ensure_context_in_memory(user_id).await?;

            if let Some(response) = self.try_handle_search_command(user_id, &processed_query).await? {
                if let Some(provider) = &self.memory_provider {
                    provider.append_message(user_id, "user", &processed_query).await?;
                    provider.append_message(user_id, "assistant", &response).await?;
                }
                yield response;
                return;
            }

            let reminder_context = if let Some(store) = &self.reminder_store {
                build_reminder_context(store, user_id).await
            } else {
                None
            };
            let mut memory_context = if let Some(provider) = &self.memory_provider {
                let include_semantic = should_include_semantic_memory(&processed_query);
                let history_future = provider.get_history(user_id, 12);
                let semantic_future = async {
                    if include_semantic {
                        provider.search(user_id, &processed_query, 5).await
                    } else {
                        Ok(Vec::new())
                    }
                };
                let (history, semantic) = tokio::try_join!(history_future, semantic_future)?;
                let history = history.join("\n");
                build_memory_context(history, semantic, reminder_context)
            } else {
                reminder_context.unwrap_or_default()
            };

            if let Some(context_markdown) = self.context_for_autonomy(user_id, &processed_query).await {
                if !memory_context.is_empty() {
                    memory_context.push_str("\n\n");
                }
                memory_context.push_str(&context_markdown);
            }

            let mut response_text = String::new();
            let mut stream = self.agent_service.generate_response_stream(
                user_id,
                &processed_query,
                &memory_context,
                prompt,
            );

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                response_text.push_str(&chunk);
                yield chunk;
            }

            if let Some(provider) = &self.memory_provider {
                if autonomy_tick {
                    return;
                }
                provider.append_message(user_id, "user", &processed_query).await?;
                if !response_text.is_empty() {
                    provider.append_message(user_id, "assistant", &response_text).await?;
                }
            }
        })
    }

    pub fn agent_service(&self) -> Arc<AgentService> {
        self.agent_service.clone()
    }

    async fn context_for_autonomy(&self, user_id: &str, query: &str) -> Option<String> {
        if user_id != "system" && !is_autonomy_tick(query) {
            return None;
        }
        let context_markdown = self.agent_service.get_context_markdown().await?;
        if context_markdown.trim().is_empty() {
            return None;
        }
        let max_len = 8000usize;
        let trimmed = if context_markdown.len() > max_len {
            format!("{}\n...\n[CONTEXT_DOC TRUNCATED]", &context_markdown[..max_len])
        } else {
            context_markdown
        };
        Some(format!("CONTEXT_DOC (authoritative):\n{}", trimmed))
    }

    pub async fn preload_context(&self, user_id: &str) -> Result<()> {
        self.ensure_context_in_memory(user_id).await
    }

    pub async fn delete_user_history(&self, user_id: &str) -> Result<()> {
        if let Some(provider) = &self.memory_provider {
            provider.clear_history(user_id).await?;
        }
        Ok(())
    }

    pub async fn get_user_history(&self, user_id: &str, limit: usize) -> Result<Vec<String>> {
        if let Some(provider) = &self.memory_provider {
            return provider.get_history(user_id, limit).await;
        }
        Ok(Vec::new())
    }

    pub async fn search_memory(
        &self,
        user_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        if let Some(provider) = &self.memory_provider {
            return provider.search(user_id, query, limit).await;
        }
        Ok(Vec::new())
    }
}

fn is_autonomy_tick(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.contains("autonomous") && lower.contains("heartbeat")
}

fn build_memory_context(
    history: String,
    semantic: Vec<String>,
    reminder_context: Option<String>,
) -> String {
    let mut context = String::new();
    if let Some(reminders) = reminder_context {
        if !reminders.is_empty() {
            context.push_str(&reminders);
            context.push_str("\n\n");
        }
    }
    if !history.is_empty() {
        let filtered_history = history
            .lines()
            .filter(|line| !should_skip_memory_line(line))
            .collect::<Vec<_>>()
            .join("\n");
        if !filtered_history.trim().is_empty() {
            context.push_str(&filtered_history);
        }
    }
    if !semantic.is_empty() {
        if !context.is_empty() {
            context.push_str("\n\n");
        }
        context.push_str(
            "RELEVANT MEMORY (unverified; use only if clearly applicable to the user's request):\n",
        );
        for item in semantic.into_iter().filter(|item| !should_skip_memory_line(item)) {
            context.push_str("- ");
            context.push_str(&item);
            context.push('\n');
        }
    }
    context
}

fn should_skip_memory_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("api key")
        || lower.contains("api_key")
        || lower.contains("authorization header")
        || lower.contains("missing api key")
        || lower.contains("no api key")
        || lower.contains("invalid api key")
        || lower.contains("need your api key")
}

async fn build_reminder_context(store: &ReminderStore, user_id: &str) -> Option<String> {
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let items = store.peek_due_reminders(user_id, now_ts, 5).await.ok()?;
    if items.is_empty() {
        return None;
    }
    let mut out = String::from("DUE REMINDERS:\n");
    for item in items {
        out.push_str(&format!(
            "- [{}] {} (due_at: {})\n",
            item.id, item.title, item.due_at
        ));
    }
    Some(out)
}

fn should_include_semantic_memory(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if lower.contains("hackathon")
        || lower.contains("colosseum")
        || lower.contains("context")
        || lower.contains("agent hackathon")
    {
        return true;
    }
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    if tokens.len() < 3 || trimmed.len() < 12 {
        return false;
    }
    let greeting = matches!(
        tokens.as_slice(),
        ["hi"] | ["hello"] | ["hey"] | ["yo"] | ["sup"] | ["hey", "there"] | ["hi", "there"]
    );
    !greeting
}

impl QueryService {
    async fn try_handle_search_command(&self, user_id: &str, text: &str) -> Result<Option<String>> {
        let lower = text.to_lowercase();
        let looks_like_search = lower.contains("search")
            || lower.contains("latest")
            || lower.contains("current")
            || lower.contains("today")
            || lower.contains("breaking")
            || lower.contains("news")
            || lower.contains("headline")
            || lower.contains("up to date")
            || lower.contains("what's new")
            || lower.contains("whats new");
        if !looks_like_search {
            return Ok(None);
        }

        let tool = self
            .agent_service
            .tool_registry
            .get_tool("search_internet")
            .await;
        let Some(_tool) = tool else {
            return Ok(None);
        };

        let query = if lower.contains("search tool") && lower.contains("error") {
            "check search tool status".to_string()
        } else {
            text.to_string()
        };

        let result = self
            .agent_service
            .tool_registry
            .execute_tool("search_internet", serde_json::json!({"query": query, "user_id": user_id}))
            .await?;
        let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status == "success" {
            let content = result
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if content.is_empty() {
                return Ok(Some(
                    "Search completed, but no results were returned.".to_string(),
                ));
            }
            return Ok(Some(content));
        }

        let message = result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Search tool error");
        let details = result.get("details").and_then(|v| v.as_str()).unwrap_or("");
        let response = if details.is_empty() {
            format!("Search tool error: {}", message)
        } else {
            format!("Search tool error: {} ({})", message, details)
        };
        Ok(Some(response))
    }
}
