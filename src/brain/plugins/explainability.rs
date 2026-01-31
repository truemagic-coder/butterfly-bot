use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ExplainabilityNote {
    pub explanation: String,
}

pub struct ExplainabilityBrain {
    last_note: Mutex<Option<ExplainabilityNote>>,
}

impl ExplainabilityBrain {
    pub fn new() -> Self {
        Self {
            last_note: Mutex::new(None),
        }
    }

    pub async fn last_note(&self) -> Option<ExplainabilityNote> {
        let guard = self.last_note.lock().await;
        guard.clone()
    }

    fn explain(message: &str) -> ExplainabilityNote {
        let explanation = format!("Response grounded in user input: {message}");
        ExplainabilityNote { explanation }
    }
}

#[async_trait]
impl BrainPlugin for ExplainabilityBrain {
    fn name(&self) -> &str {
        "explainability"
    }

    fn description(&self) -> &str {
        "Creates lightweight explainability notes"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let note = Self::explain(&text);
            let mut guard = self.last_note.lock().await;
            *guard = Some(note);
        }
        Ok(())
    }
}
