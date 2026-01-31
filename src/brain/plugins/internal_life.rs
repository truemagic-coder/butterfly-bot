use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct InternalLifeSignal {
    pub insight: String,
    pub curiosity_topic: String,
    pub confidence: f32,
}

pub struct InternalLifeBrain {
    last_signal: Mutex<Option<InternalLifeSignal>>,
}

impl InternalLifeBrain {
    pub fn new() -> Self {
        Self {
            last_signal: Mutex::new(None),
        }
    }

    pub async fn last_signal(&self) -> Option<InternalLifeSignal> {
        let guard = self.last_signal.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> InternalLifeSignal {
        let lower = message.to_lowercase();
        let curiosity_topic = if lower.contains("pattern") {
            "pattern_observation"
        } else if lower.contains("learn") || lower.contains("improve") {
            "self_improvement"
        } else {
            "user_interaction"
        };
        InternalLifeSignal {
            insight: "Captured a lightweight internal reflection".to_string(),
            curiosity_topic: curiosity_topic.to_string(),
            confidence: 0.6,
        }
    }
}

#[async_trait]
impl BrainPlugin for InternalLifeBrain {
    fn name(&self) -> &str {
        "internal_life"
    }

    fn description(&self) -> &str {
        "Tracks lightweight internal experience and curiosity"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let signal = Self::analyze(&text);
            let mut guard = self.last_signal.lock().await;
            *guard = Some(signal);
        }
        Ok(())
    }
}
