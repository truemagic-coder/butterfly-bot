use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct NarrativeSummary {
    pub response: String,
    pub events_logged: u32,
}

pub struct NarrativeIdentityBrain {
    last_summary: Mutex<Option<NarrativeSummary>>,
    event_count: Mutex<u32>,
}

impl NarrativeIdentityBrain {
    pub fn new() -> Self {
        Self {
            last_summary: Mutex::new(None),
            event_count: Mutex::new(0),
        }
    }

    pub async fn last_summary(&self) -> Option<NarrativeSummary> {
        let guard = self.last_summary.lock().await;
        guard.clone()
    }

    fn is_evolution_query(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["why did you change", "how have you changed", "what changed"]
            .iter()
            .any(|kw| lower.contains(kw))
    }
}

#[async_trait]
impl BrainPlugin for NarrativeIdentityBrain {
    fn name(&self) -> &str {
        "narrative_identity"
    }

    fn description(&self) -> &str {
        "Tracks identity evolution signals"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        match event {
            BrainEvent::UserMessage { text, .. } => {
                if Self::is_evolution_query(&text) {
                    let count = { *self.event_count.lock().await };
                    let summary = NarrativeSummary {
                        response: "Identity evolution tracked across updates".to_string(),
                        events_logged: count,
                    };
                    let mut guard = self.last_summary.lock().await;
                    *guard = Some(summary);
                }
            }
            BrainEvent::Start => {
                let mut guard = self.event_count.lock().await;
                *guard += 1;
            }
            _ => {}
        }
        Ok(())
    }
}
