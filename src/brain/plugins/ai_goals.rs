use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct AiGoal {
    pub goal_statement: String,
    pub category: String,
    pub confidence: f32,
}

pub struct AiGoalsBrain {
    last_goal: Mutex<Option<AiGoal>>,
}

impl AiGoalsBrain {
    pub fn new() -> Self {
        Self {
            last_goal: Mutex::new(None),
        }
    }

    pub async fn last_goal(&self) -> Option<AiGoal> {
        let guard = self.last_goal.lock().await;
        guard.clone()
    }

    fn derive_goal(message: &str) -> AiGoal {
        let lower = message.to_lowercase();
        if lower.contains("learn") || lower.contains("study") {
            return AiGoal {
                goal_statement: "Help the user learn effectively".to_string(),
                category: "learning".to_string(),
                confidence: 0.6,
            };
        }
        if lower.contains("plan") || lower.contains("goal") {
            return AiGoal {
                goal_statement: "Support the user with planning".to_string(),
                category: "productivity".to_string(),
                confidence: 0.6,
            };
        }
        AiGoal {
            goal_statement: "Provide supportive, engaging conversations".to_string(),
            category: "support".to_string(),
            confidence: 0.5,
        }
    }
}

#[async_trait]
impl BrainPlugin for AiGoalsBrain {
    fn name(&self) -> &str {
        "ai_goals"
    }

    fn description(&self) -> &str {
        "Tracks a lightweight relationship goal for the AI"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let goal = Self::derive_goal(&text);
            let mut guard = self.last_goal.lock().await;
            *guard = Some(goal);
        }
        Ok(())
    }
}
