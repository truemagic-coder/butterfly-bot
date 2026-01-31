use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PurposeSignal {
    pub mission_alignment: String,
    pub chaos_score: f32,
    pub ai_emotion: String,
}

pub struct PurposeBrain {
    last_signal: Mutex<Option<PurposeSignal>>,
}

impl PurposeBrain {
    pub fn new() -> Self {
        Self {
            last_signal: Mutex::new(None),
        }
    }

    pub async fn last_signal(&self) -> Option<PurposeSignal> {
        let guard = self.last_signal.lock().await;
        guard.clone()
    }

    fn evaluate(message: &str) -> PurposeSignal {
        let lower = message.to_lowercase();
        let chaos_score = if lower.contains("confused") || lower.contains("stuck") {
            7.0
        } else {
            3.0
        };
        let mission_alignment = if chaos_score >= 7.0 { "good" } else { "fair" };
        let ai_emotion = if chaos_score >= 7.0 {
            "urgency"
        } else {
            "satisfaction"
        };
        PurposeSignal {
            mission_alignment: mission_alignment.to_string(),
            chaos_score,
            ai_emotion: ai_emotion.to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for PurposeBrain {
    fn name(&self) -> &str {
        "purpose"
    }

    fn description(&self) -> &str {
        "Tracks mission alignment and chaos signals"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let signal = Self::evaluate(&text);
            let mut guard = self.last_signal.lock().await;
            *guard = Some(signal);
        }
        Ok(())
    }
}
