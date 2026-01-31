use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};
use crate::interfaces::providers::LlmProvider;
use crate::providers::openai::OpenAiProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AbstractionType {
    Algorithmic,
    Geometric,
    Logical,
    Social,
    Temporal,
    Compositional,
}

impl AbstractionType {
    fn from_str(value: &str) -> Self {
        match value {
            "geometric" => Self::Geometric,
            "logical" => Self::Logical,
            "social" => Self::Social,
            "temporal" => Self::Temporal,
            "compositional" => Self::Compositional,
            _ => Self::Algorithmic,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AbstractionDomain {
    Universal,
    Mathematics,
    Language,
    Reasoning,
    Planning,
    SocialInteraction,
}

impl AbstractionDomain {
    fn from_str(value: &str) -> Self {
        match value {
            "mathematics" => Self::Mathematics,
            "language" => Self::Language,
            "reasoning" => Self::Reasoning,
            "planning" => Self::Planning,
            "social_interaction" => Self::SocialInteraction,
            _ => Self::Universal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Abstraction {
    pub id: String,
    pub name: String,
    pub abstraction_type: AbstractionType,
    pub domains: Vec<AbstractionDomain>,
    pub description: String,
    pub pattern_code: String,
    pub usage_count: u64,
    pub success_rate: f32,
    pub extracted_from: Vec<String>,
    pub applies_to: Vec<String>,
    pub composition_rules: HashMap<String, Value>,
    pub created_at: String,
    pub last_used: String,
}

#[derive(Debug, Clone, Default)]
pub struct AbstractionLibrary {
    pub abstractions: HashMap<String, Abstraction>,
    pub composition_graph: HashMap<String, Vec<String>>,
    pub usage_stats: HashMap<String, u64>,
    pub total_extractions: u64,
    pub total_applications: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AbstractionExtractionConfig {
    pub auto_extract: bool,
    pub model: Option<String>,
    pub rank_model: Option<String>,
    pub task_type: Option<String>,
}

pub struct AbstractionExtractionBrain {
    config: AbstractionExtractionConfig,
    openai: Option<Arc<dyn LlmProvider>>,
    library: Mutex<AbstractionLibrary>,
    last_user_message: Mutex<HashMap<String, String>>,
}

impl AbstractionExtractionBrain {
    pub fn new(config: Value) -> Self {
        let openai = build_openai_provider(&config);
        let extraction_config = parse_config(&config);
        Self {
            config: extraction_config,
            openai,
            library: Mutex::new(AbstractionLibrary::default()),
            last_user_message: Mutex::new(HashMap::new()),
        }
    }

    async fn handle_user_message(&self, ctx: &BrainContext, text: &str) {
        let mut guard = self.last_user_message.lock().await;
        guard.insert(ctx.user_id.clone().unwrap_or_default(), text.to_string());
    }

    async fn handle_assistant_response(&self, ctx: &BrainContext, text: &str) -> Result<()> {
        if !self.config.auto_extract {
            return Ok(());
        }
        let user_id = ctx.user_id.clone().unwrap_or_default();
        let mut last_user = self.last_user_message.lock().await;
        let Some(task) = last_user.remove(&user_id) else {
            return Ok(());
        };

        let task_type = self
            .config
            .task_type
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let _ = self
            .extract_abstraction(&user_id, &task, text, &task_type)
            .await?;
        Ok(())
    }

    async fn extract_abstraction(
        &self,
        user_id: &str,
        task: &str,
        solution: &str,
        task_type: &str,
    ) -> Result<Option<Abstraction>> {
        let Some(openai) = &self.openai else {
            return Ok(None);
        };

        let prompt = format!(
            "Analyze this successful task solution and extract the underlying abstract pattern:\n\nTask: {task}\nSolution: {solution}\nTask Type: {task_type}\n\nExtract:\n1. The core abstract pattern\n2. The type of abstraction (algorithmic, logical, geometric, etc.)\n3. Which domains it applies to\n4. How it could be reused for similar problems\n5. A general description of the pattern\n\nReturn JSON with fields: name, type, domains, description, pattern_code, applies_to"
        );

        let response = openai
            .generate_text(&prompt, "", None)
            .await
            .unwrap_or_default();
        let data: Value = serde_json::from_str(&response).unwrap_or(Value::Null);

        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("abstraction")
            .to_string();
        let abstraction_type = data
            .get("type")
            .and_then(|v| v.as_str())
            .map(AbstractionType::from_str)
            .unwrap_or(AbstractionType::Algorithmic);
        let domains = data
            .get("domains")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(AbstractionDomain::from_str))
                    .collect()
            })
            .unwrap_or_else(|| vec![AbstractionDomain::Universal]);
        let description = data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let pattern_code = data
            .get("pattern_code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let applies_to = data
            .get("applies_to")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| vec![task_type.to_string()]);

        let abstraction_id = hash_id(&format!("{name}_{task_type}"));
        let now = now_string();
        let created_at = now.clone();
        let mut library = self.library.lock().await;

        let mut updated_existing: Option<Abstraction> = None;
        if let Some(existing) = library.abstractions.get_mut(&abstraction_id) {
            existing.usage_count += 1;
            existing.last_used = now.clone();
            existing.extracted_from.push(format!("{user_id}_{task}"));
            updated_existing = Some(existing.clone());
        }
        if let Some(updated) = updated_existing {
            library.total_extractions += 1;
            return Ok(Some(updated));
        }

        let abstraction = Abstraction {
            id: abstraction_id.clone(),
            name,
            abstraction_type,
            domains,
            description,
            pattern_code,
            usage_count: 1,
            success_rate: 1.0,
            extracted_from: vec![format!("{user_id}_{task}")],
            applies_to,
            composition_rules: HashMap::new(),
            created_at,
            last_used: now,
        };

        library
            .abstractions
            .insert(abstraction_id, abstraction.clone());
        library.total_extractions += 1;
        Ok(Some(abstraction))
    }

    pub async fn abstraction_count(&self) -> usize {
        let library = self.library.lock().await;
        library.abstractions.len()
    }
}

#[async_trait]
impl BrainPlugin for AbstractionExtractionBrain {
    fn name(&self) -> &str {
        "abstraction_extraction"
    }

    fn description(&self) -> &str {
        "Extract reusable abstractions from successful solutions"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        match event {
            BrainEvent::UserMessage { text, .. } => {
                self.handle_user_message(ctx, &text).await;
            }
            BrainEvent::AssistantResponse { text, .. } => {
                self.handle_assistant_response(ctx, &text).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn hash_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn parse_config(config: &Value) -> AbstractionExtractionConfig {
    let mut out = AbstractionExtractionConfig::default();
    if let Some(brains) = config.get("brains") {
        if let Some(entries) = brains.as_array() {
            for entry in entries {
                if let Some(obj) = entry.as_object() {
                    let name = obj
                        .get("name")
                        .or_else(|| obj.get("class"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if name != "abstraction_extraction" {
                        continue;
                    }
                    if let Some(cfg) = obj.get("config") {
                        if let Some(auto) = cfg.get("auto_extract").and_then(|v| v.as_bool()) {
                            out.auto_extract = auto;
                        }
                        if let Some(model) = cfg.get("model").and_then(|v| v.as_str()) {
                            out.model = Some(model.to_string());
                        }
                        if let Some(rank) = cfg.get("rank_model").and_then(|v| v.as_str()) {
                            out.rank_model = Some(rank.to_string());
                        }
                        if let Some(task_type) = cfg.get("task_type").and_then(|v| v.as_str()) {
                            out.task_type = Some(task_type.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

fn build_openai_provider(config: &Value) -> Option<Arc<dyn LlmProvider>> {
    let openai = config.get("openai")?;
    let api_key = openai.get("api_key")?.as_str()?.to_string();
    let model = openai
        .get("model")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string());
    let base_url = openai
        .get("base_url")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string());
    Some(Arc::new(OpenAiProvider::new(api_key, model, base_url)))
}

#[cfg(test)]
mod tests {
    use super::AbstractionExtractionBrain;
    use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};
    use serde_json::json;

    #[tokio::test]
    async fn does_not_extract_without_openai() {
        let plugin = AbstractionExtractionBrain::new(json!({
            "brains": [
                {"name": "abstraction_extraction", "config": {"auto_extract": true}}
            ]
        }));
        let ctx = BrainContext {
            agent_name: "agent".to_string(),
            user_id: Some("u1".to_string()),
        };
        plugin
            .on_event(
                BrainEvent::UserMessage {
                    user_id: "u1".to_string(),
                    text: "task".to_string(),
                },
                &ctx,
            )
            .await
            .unwrap();
        plugin
            .on_event(
                BrainEvent::AssistantResponse {
                    user_id: "u1".to_string(),
                    text: "solution".to_string(),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(plugin.abstraction_count().await, 0);
    }
}
