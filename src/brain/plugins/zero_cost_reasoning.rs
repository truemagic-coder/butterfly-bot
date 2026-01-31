use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ZeroCostReasoningResult {
    pub handled: bool,
    pub note: String,
}

pub struct ZeroCostReasoningBrain {
    last_result: Mutex<Option<ZeroCostReasoningResult>>,
}

impl ZeroCostReasoningBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<ZeroCostReasoningResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn should_handle(message: &str) -> bool {
        let lower = message.to_lowercase();
        let patterns = [
            "i keep",
            "i always",
            "every time",
            "whenever",
            "procrastinate",
            "anxious about",
            "struggle with",
            "how do i",
            "what should i do",
            "i'm stuck",
        ];
        patterns.iter().any(|token| lower.contains(token))
    }
}

#[async_trait]
impl BrainPlugin for ZeroCostReasoningBrain {
    fn name(&self) -> &str {
        "zero_cost_reasoning"
    }

    fn description(&self) -> &str {
        "Classical pattern heuristics for low-cost reasoning"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let handled = Self::should_handle(&text);
            let note = if handled {
                "Pattern matched by zero-cost heuristics".to_string()
            } else {
                "No match".to_string()
            };
            let mut guard = self.last_result.lock().await;
            *guard = Some(ZeroCostReasoningResult { handled, note });
        }
        Ok(())
    }
}
