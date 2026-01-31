use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ProactiveObservation {
    pub observation: String,
    pub priority: String,
}

pub struct ProactiveAwarenessBrain {
    last_observation: Mutex<Option<ProactiveObservation>>,
}

impl ProactiveAwarenessBrain {
    pub fn new() -> Self {
        Self {
            last_observation: Mutex::new(None),
        }
    }

    pub async fn last_observation(&self) -> Option<ProactiveObservation> {
        let guard = self.last_observation.lock().await;
        guard.clone()
    }

    fn observe(message: &str) -> Option<ProactiveObservation> {
        let lower = message.to_lowercase();
        if lower.contains("last time") || lower.contains("remember") {
            return Some(ProactiveObservation {
                observation: "Check-in on a previously mentioned topic".to_string(),
                priority: "medium".to_string(),
            });
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for ProactiveAwarenessBrain {
    fn name(&self) -> &str {
        "proactive_awareness"
    }

    fn description(&self) -> &str {
        "Suggests proactive check-ins during a conversation"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let observation = Self::observe(&text);
            let mut guard = self.last_observation.lock().await;
            *guard = observation;
        }
        Ok(())
    }
}
