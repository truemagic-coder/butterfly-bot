use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct FirstImpressionNote {
    pub guidance: String,
}

pub struct FirstImpressionCoachBrain {
    last_note: Mutex<Option<FirstImpressionNote>>,
}

impl FirstImpressionCoachBrain {
    pub fn new() -> Self {
        Self {
            last_note: Mutex::new(None),
        }
    }

    pub async fn last_note(&self) -> Option<FirstImpressionNote> {
        let guard = self.last_note.lock().await;
        guard.clone()
    }

    fn build(message: &str) -> FirstImpressionNote {
        let lower = message.to_lowercase();
        let guidance = if lower.contains("introduce") || lower.contains("first impression") {
            "Lead with warmth and a crisp summary".to_string()
        } else {
            "Keep the opening concise and friendly".to_string()
        };
        FirstImpressionNote { guidance }
    }
}

#[async_trait]
impl BrainPlugin for FirstImpressionCoachBrain {
    fn name(&self) -> &str {
        "first_impression_coach"
    }

    fn description(&self) -> &str {
        "Offers first-impression coaching prompts"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let note = Self::build(&text);
            let mut guard = self.last_note.lock().await;
            *guard = Some(note);
        }
        Ok(())
    }
}
