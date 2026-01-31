use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct OptimizationHint {
    pub category: String,
    pub suggested_change: String,
}

pub struct SelfOptimizationBrain {
    last_hint: Mutex<Option<OptimizationHint>>,
}

impl SelfOptimizationBrain {
    pub fn new() -> Self {
        Self {
            last_hint: Mutex::new(None),
        }
    }

    pub async fn last_hint(&self) -> Option<OptimizationHint> {
        let guard = self.last_hint.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> OptimizationHint {
        let lower = message.to_lowercase();
        if lower.contains("too long") {
            return OptimizationHint {
                category: "response_length".to_string(),
                suggested_change: "shorten responses".to_string(),
            };
        }
        OptimizationHint {
            category: "tone_formality".to_string(),
            suggested_change: "maintain current tone".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for SelfOptimizationBrain {
    fn name(&self) -> &str {
        "self_optimization"
    }

    fn description(&self) -> &str {
        "Suggests lightweight optimization hints"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let hint = Self::analyze(&text);
            let mut guard = self.last_hint.lock().await;
            *guard = Some(hint);
        }
        Ok(())
    }
}
