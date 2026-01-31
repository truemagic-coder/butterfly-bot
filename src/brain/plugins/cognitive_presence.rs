use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct CognitiveBalance {
    pub wit_ratio: f32,
    pub context: String,
}

pub struct CognitivePresenceBrain {
    last_balance: Mutex<Option<CognitiveBalance>>,
}

impl CognitivePresenceBrain {
    pub fn new() -> Self {
        Self {
            last_balance: Mutex::new(None),
        }
    }

    pub async fn last_balance(&self) -> Option<CognitiveBalance> {
        let guard = self.last_balance.lock().await;
        guard.clone()
    }

    fn classify_context(message: &str) -> CognitiveBalance {
        let lower = message.to_lowercase();
        if lower.contains("why") || lower.contains("explain") || lower.contains("how") {
            return CognitiveBalance {
                wit_ratio: 0.35,
                context: "explanation".to_string(),
            };
        }
        if lower.contains("brainstorm") || lower.contains("ideas") {
            return CognitiveBalance {
                wit_ratio: 0.65,
                context: "creative".to_string(),
            };
        }
        CognitiveBalance {
            wit_ratio: 0.5,
            context: "casual".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for CognitivePresenceBrain {
    fn name(&self) -> &str {
        "cognitive_presence"
    }

    fn description(&self) -> &str {
        "Suggests wit/wisdom balance based on context"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let balance = Self::classify_context(&text);
            let mut guard = self.last_balance.lock().await;
            *guard = Some(balance);
        }
        Ok(())
    }
}
