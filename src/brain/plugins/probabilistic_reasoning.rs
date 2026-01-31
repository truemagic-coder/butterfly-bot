use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ProbabilisticResult {
    pub probability: f32,
    pub confidence_interval: (f32, f32),
    pub reasoning: String,
}

pub struct ProbabilisticReasoningBrain {
    last_result: Mutex<Option<ProbabilisticResult>>,
}

impl ProbabilisticReasoningBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<ProbabilisticResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn involves_uncertainty(message: &str) -> bool {
        let lower = message.to_lowercase();
        [
            "probability",
            "chance",
            "likely",
            "risk",
            "odds",
            "uncertain",
        ]
        .iter()
        .any(|token| lower.contains(token))
    }

    fn naive_estimate(message: &str) -> ProbabilisticResult {
        let lower = message.to_lowercase();
        let base = if lower.contains("unlikely") {
            0.25
        } else if lower.contains("likely") {
            0.7
        } else {
            0.5
        };
        ProbabilisticResult {
            probability: base,
            confidence_interval: (base - 0.1, base + 0.1),
            reasoning: "Naive uncertainty estimate based on language".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for ProbabilisticReasoningBrain {
    fn name(&self) -> &str {
        "probabilistic_reasoning"
    }

    fn description(&self) -> &str {
        "Provides simple probabilistic estimates from language cues"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::involves_uncertainty(&text) {
                let result = Self::naive_estimate(&text);
                let mut guard = self.last_result.lock().await;
                *guard = Some(result);
            }
        }
        Ok(())
    }
}
