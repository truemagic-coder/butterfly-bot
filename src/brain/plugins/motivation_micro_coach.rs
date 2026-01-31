use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct MotivationPrompt {
    pub encouragement: String,
    pub action: String,
}

pub struct MotivationMicroCoachBrain {
    last_prompt: Mutex<Option<MotivationPrompt>>,
}

impl MotivationMicroCoachBrain {
    pub fn new() -> Self {
        Self {
            last_prompt: Mutex::new(None),
        }
    }

    pub async fn last_prompt(&self) -> Option<MotivationPrompt> {
        let guard = self.last_prompt.lock().await;
        guard.clone()
    }

    fn coach(message: &str) -> MotivationPrompt {
        let lower = message.to_lowercase();
        let encouragement = if lower.contains("tired") || lower.contains("burned out") {
            "It's okay to go slower—momentum still counts".to_string()
        } else {
            "You can make progress with one small step".to_string()
        };
        MotivationPrompt {
            encouragement,
            action: "Pick one five-minute action".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for MotivationMicroCoachBrain {
    fn name(&self) -> &str {
        "motivation_micro_coach"
    }

    fn description(&self) -> &str {
        "Delivers short motivational nudges"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let prompt = Self::coach(&text);
            let mut guard = self.last_prompt.lock().await;
            *guard = Some(prompt);
        }
        Ok(())
    }
}
