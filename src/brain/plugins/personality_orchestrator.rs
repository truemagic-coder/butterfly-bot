use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PersonalityDirective {
    pub style: String,
    pub note: String,
}

pub struct PersonalityOrchestratorBrain {
    last_directive: Mutex<Option<PersonalityDirective>>,
}

impl PersonalityOrchestratorBrain {
    pub fn new() -> Self {
        Self {
            last_directive: Mutex::new(None),
        }
    }

    pub async fn last_directive(&self) -> Option<PersonalityDirective> {
        let guard = self.last_directive.lock().await;
        guard.clone()
    }

    fn select_style(message: &str) -> PersonalityDirective {
        let lower = message.to_lowercase();
        let style = if lower.contains("urgent") {
            "decisive".to_string()
        } else if lower.contains("unsure") || lower.contains("confused") {
            "gentle".to_string()
        } else {
            "balanced".to_string()
        };
        PersonalityDirective {
            style,
            note: "Adapt tone to user state".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for PersonalityOrchestratorBrain {
    fn name(&self) -> &str {
        "personality_orchestrator"
    }

    fn description(&self) -> &str {
        "Selects a response style based on context"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let directive = Self::select_style(&text);
            let mut guard = self.last_directive.lock().await;
            *guard = Some(directive);
        }
        Ok(())
    }
}
