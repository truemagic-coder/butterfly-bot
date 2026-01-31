use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PersonalitySignal {
    pub maxim: String,
    pub anger_level: String,
}

pub struct PersonalityBrain {
    last_signal: Mutex<Option<PersonalitySignal>>,
}

impl PersonalityBrain {
    pub fn new() -> Self {
        Self {
            last_signal: Mutex::new(None),
        }
    }

    pub async fn last_signal(&self) -> Option<PersonalitySignal> {
        let guard = self.last_signal.lock().await;
        guard.clone()
    }

    fn detect(message: &str) -> PersonalitySignal {
        let lower = message.to_lowercase();
        let anger_level = if lower.contains("insult") || lower.contains("stupid") {
            "warning"
        } else {
            "none"
        };
        let maxim = "Strong beliefs, loosely held".to_string();
        PersonalitySignal {
            maxim,
            anger_level: anger_level.to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for PersonalityBrain {
    fn name(&self) -> &str {
        "personality"
    }

    fn description(&self) -> &str {
        "Emits a lightweight personality signal (maxim + boundary tone)"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let signal = Self::detect(&text);
            let mut guard = self.last_signal.lock().await;
            *guard = Some(signal);
        }
        Ok(())
    }
}
