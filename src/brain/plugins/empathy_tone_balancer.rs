use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EmpathyBalance {
    pub level: String,
    pub guidance: String,
}

pub struct EmpathyToneBalancerBrain {
    last_balance: Mutex<Option<EmpathyBalance>>,
}

impl EmpathyToneBalancerBrain {
    pub fn new() -> Self {
        Self {
            last_balance: Mutex::new(None),
        }
    }

    pub async fn last_balance(&self) -> Option<EmpathyBalance> {
        let guard = self.last_balance.lock().await;
        guard.clone()
    }

    fn balance(message: &str) -> EmpathyBalance {
        let lower = message.to_lowercase();
        if lower.contains("sad") || lower.contains("hurt") {
            return EmpathyBalance {
                level: "high".to_string(),
                guidance: "Lead with validation before advice".to_string(),
            };
        }
        EmpathyBalance {
            level: "moderate".to_string(),
            guidance: "Blend empathy with action steps".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for EmpathyToneBalancerBrain {
    fn name(&self) -> &str {
        "empathy_tone_balancer"
    }

    fn description(&self) -> &str {
        "Balances empathy level and guidance"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let balance = Self::balance(&text);
            let mut guard = self.last_balance.lock().await;
            *guard = Some(balance);
        }
        Ok(())
    }
}
