use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct CoordinationDecision {
    pub strategy: String,
    pub agents_needed: u8,
}

pub struct MultiAgentCoordinationBrain {
    last_decision: Mutex<Option<CoordinationDecision>>,
}

impl MultiAgentCoordinationBrain {
    pub fn new() -> Self {
        Self {
            last_decision: Mutex::new(None),
        }
    }

    pub async fn last_decision(&self) -> Option<CoordinationDecision> {
        let guard = self.last_decision.lock().await;
        guard.clone()
    }

    fn decide(message: &str) -> CoordinationDecision {
        let lower = message.to_lowercase();
        if lower.contains("complex") || lower.contains("multi-step") {
            return CoordinationDecision {
                strategy: "parallel".to_string(),
                agents_needed: 3,
            };
        }
        CoordinationDecision {
            strategy: "single".to_string(),
            agents_needed: 1,
        }
    }
}

#[async_trait]
impl BrainPlugin for MultiAgentCoordinationBrain {
    fn name(&self) -> &str {
        "multi_agent_coordination"
    }

    fn description(&self) -> &str {
        "Suggests when to use multiple agents"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let decision = Self::decide(&text);
            let mut guard = self.last_decision.lock().await;
            *guard = Some(decision);
        }
        Ok(())
    }
}
