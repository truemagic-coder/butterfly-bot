use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct CausalModel {
    pub situation: String,
    pub outcome: String,
    pub root_causes: Vec<String>,
    pub intervention_points: Vec<String>,
}

pub struct CausalReasoningBrain {
    last_model: Mutex<Option<CausalModel>>,
}

impl CausalReasoningBrain {
    pub fn new() -> Self {
        Self {
            last_model: Mutex::new(None),
        }
    }

    pub async fn last_model(&self) -> Option<CausalModel> {
        let guard = self.last_model.lock().await;
        guard.clone()
    }

    fn involves_causation(message: &str) -> bool {
        let lower = message.to_lowercase();
        [
            "why", "because", "cause", "effect", "result", "lead to", "what if",
        ]
        .iter()
        .any(|token| lower.contains(token))
    }

    fn build_model(message: &str) -> CausalModel {
        let parts: Vec<&str> = message.splitn(2, "because").collect();
        let (outcome, cause) = if parts.len() == 2 {
            (parts[0].trim(), parts[1].trim())
        } else {
            (message, "unknown cause")
        };
        CausalModel {
            situation: message.to_string(),
            outcome: outcome.to_string(),
            root_causes: vec![cause.to_string()],
            intervention_points: vec!["Identify a controllable root cause".to_string()],
        }
    }
}

#[async_trait]
impl BrainPlugin for CausalReasoningBrain {
    fn name(&self) -> &str {
        "causal_reasoning"
    }

    fn description(&self) -> &str {
        "Builds simple causal models from user statements"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::involves_causation(&text) {
                let model = Self::build_model(&text);
                let mut guard = self.last_model.lock().await;
                *guard = Some(model);
            }
        }
        Ok(())
    }
}
