use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct FormattingResult {
    pub formatted: String,
    pub applied: bool,
}

pub struct ResponseFormatterBrain {
    last_result: Mutex<Option<FormattingResult>>,
}

impl ResponseFormatterBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<FormattingResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn format(text: &str) -> FormattingResult {
        let formatted = text.replace(". ", ".\n\n");
        FormattingResult {
            formatted,
            applied: true,
        }
    }
}

#[async_trait]
impl BrainPlugin for ResponseFormatterBrain {
    fn name(&self) -> &str {
        "response_formatter"
    }

    fn description(&self) -> &str {
        "Applies lightweight formatting to assistant responses"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::AssistantResponse { text, .. } = event {
            let result = Self::format(&text);
            let mut guard = self.last_result.lock().await;
            *guard = Some(result);
        }
        Ok(())
    }
}
