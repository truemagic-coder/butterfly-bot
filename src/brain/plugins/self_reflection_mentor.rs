use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ReflectionPrompt {
    pub question: String,
}

pub struct SelfReflectionMentorBrain {
    last_prompt: Mutex<Option<ReflectionPrompt>>,
}

impl SelfReflectionMentorBrain {
    pub fn new() -> Self {
        Self {
            last_prompt: Mutex::new(None),
        }
    }

    pub async fn last_prompt(&self) -> Option<ReflectionPrompt> {
        let guard = self.last_prompt.lock().await;
        guard.clone()
    }

    fn prompt(message: &str) -> ReflectionPrompt {
        let lower = message.to_lowercase();
        let question = if lower.contains("why") {
            "What value matters most to you here?".to_string()
        } else {
            "What feels most important about this situation?".to_string()
        };
        ReflectionPrompt { question }
    }
}

#[async_trait]
impl BrainPlugin for SelfReflectionMentorBrain {
    fn name(&self) -> &str {
        "self_reflection_mentor"
    }

    fn description(&self) -> &str {
        "Prompts self-reflection questions"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let prompt = Self::prompt(&text);
            let mut guard = self.last_prompt.lock().await;
            *guard = Some(prompt);
        }
        Ok(())
    }
}
