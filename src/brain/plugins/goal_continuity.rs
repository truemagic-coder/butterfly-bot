use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct GoalContinuitySignal {
    pub goal: String,
    pub follow_up: String,
}

pub struct GoalContinuityBrain {
    last_signal: Mutex<Option<GoalContinuitySignal>>,
}

impl GoalContinuityBrain {
    pub fn new() -> Self {
        Self {
            last_signal: Mutex::new(None),
        }
    }

    pub async fn last_signal(&self) -> Option<GoalContinuitySignal> {
        let guard = self.last_signal.lock().await;
        guard.clone()
    }

    fn extract_goal(message: &str) -> GoalContinuitySignal {
        let lower = message.to_lowercase();
        let goal = if lower.contains("goal") {
            "current goal".to_string()
        } else if lower.contains("working on") {
            "ongoing effort".to_string()
        } else {
            "primary focus".to_string()
        };
        GoalContinuitySignal {
            goal,
            follow_up: "Ask for progress on the last stated objective".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for GoalContinuityBrain {
    fn name(&self) -> &str {
        "goal_continuity"
    }

    fn description(&self) -> &str {
        "Tracks ongoing goals and suggests follow-ups"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let signal = Self::extract_goal(&text);
            let mut guard = self.last_signal.lock().await;
            *guard = Some(signal);
        }
        Ok(())
    }
}
