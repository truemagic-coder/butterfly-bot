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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interfaces::brain::{BrainContext, BrainPlugin};

    #[test]
    fn decide_routes_complex_prompts_to_parallel_strategy() {
        let simple = MultiAgentCoordinationBrain::decide("draft this note");
        assert_eq!(simple.strategy, "single");
        assert_eq!(simple.agents_needed, 1);

        let complex = MultiAgentCoordinationBrain::decide("run a complex multi-step migration");
        assert_eq!(complex.strategy, "parallel");
        assert_eq!(complex.agents_needed, 3);
    }

    #[tokio::test]
    async fn user_message_updates_last_decision() {
        let plugin = MultiAgentCoordinationBrain::new();
        let ctx = BrainContext {
            agent_name: "agent".to_string(),
            user_id: Some("u1".to_string()),
        };

        plugin
            .on_event(BrainEvent::Tick, &ctx)
            .await
            .expect("tick should be ignored safely");
        assert!(plugin.last_decision().await.is_none());

        plugin
            .on_event(
                BrainEvent::UserMessage {
                    user_id: "u1".to_string(),
                    text: "this is complex and multi-step".to_string(),
                },
                &ctx,
            )
            .await
            .expect("user message should produce a decision");

        let decision = plugin.last_decision().await.expect("decision recorded");
        assert_eq!(decision.strategy, "parallel");
        assert_eq!(decision.agents_needed, 3);
    }
}
