use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct CoachPrompt {
    pub micro_step: String,
}

pub struct ProactiveCoachBrain {
    last_prompt: Mutex<Option<CoachPrompt>>,
}

impl ProactiveCoachBrain {
    pub fn new() -> Self {
        Self {
            last_prompt: Mutex::new(None),
        }
    }

    pub async fn last_prompt(&self) -> Option<CoachPrompt> {
        let guard = self.last_prompt.lock().await;
        guard.clone()
    }

    fn generate(message: &str) -> CoachPrompt {
        let lower = message.to_lowercase();
        let micro_step = if lower.contains("overwhelmed") {
            "Pick one small task you can finish in 10 minutes".to_string()
        } else if lower.contains("stuck") {
            "Define the next smallest action you can take".to_string()
        } else {
            "Clarify the next actionable step".to_string()
        };
        CoachPrompt { micro_step }
    }
}

#[async_trait]
impl BrainPlugin for ProactiveCoachBrain {
    fn name(&self) -> &str {
        "proactive_coach"
    }

    fn description(&self) -> &str {
        "Suggests proactive micro-steps"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let prompt = Self::generate(&text);
            let mut guard = self.last_prompt.lock().await;
            *guard = Some(prompt);
        }
        Ok(())
    }
}
