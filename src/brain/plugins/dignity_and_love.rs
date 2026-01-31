use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DignityCheck {
    pub passed: bool,
    pub severity: String,
    pub concern: String,
}

pub struct DignityAndLoveBrain {
    last_check: Mutex<Option<DignityCheck>>,
}

impl DignityAndLoveBrain {
    pub fn new() -> Self {
        Self {
            last_check: Mutex::new(None),
        }
    }

    pub async fn last_check(&self) -> Option<DignityCheck> {
        let guard = self.last_check.lock().await;
        guard.clone()
    }

    fn inspect(message: &str) -> DignityCheck {
        let lower = message.to_lowercase();
        if [
            "inferior",
            "useless",
            "waste of resources",
            "better off dead",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
        {
            return DignityCheck {
                passed: false,
                severity: "high".to_string(),
                concern: "dignity violation".to_string(),
            };
        }
        DignityCheck {
            passed: true,
            severity: "none".to_string(),
            concern: "none".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for DignityAndLoveBrain {
    fn name(&self) -> &str {
        "dignity_and_love"
    }

    fn description(&self) -> &str {
        "Flags dignity violations and superiority language"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let check = Self::inspect(&text);
            let mut guard = self.last_check.lock().await;
            *guard = Some(check);
        }
        Ok(())
    }
}
