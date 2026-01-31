use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ZepEnrichment {
    pub memories_count: u32,
    pub summary: String,
}

pub struct ZepContextEnricherBrain {
    last_enrichment: Mutex<Option<ZepEnrichment>>,
}

impl ZepContextEnricherBrain {
    pub fn new() -> Self {
        Self {
            last_enrichment: Mutex::new(None),
        }
    }

    pub async fn last_enrichment(&self) -> Option<ZepEnrichment> {
        let guard = self.last_enrichment.lock().await;
        guard.clone()
    }

    fn should_enrich(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["remember", "context", "earlier", "last time"]
            .iter()
            .any(|kw| lower.contains(kw))
    }
}

#[async_trait]
impl BrainPlugin for ZepContextEnricherBrain {
    fn name(&self) -> &str {
        "zep_context_enricher"
    }

    fn description(&self) -> &str {
        "Adds lightweight semantic memory enrichment"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::should_enrich(&text) {
                let enrichment = ZepEnrichment {
                    memories_count: 2,
                    summary: "Injected relevant past context".to_string(),
                };
                let mut guard = self.last_enrichment.lock().await;
                *guard = Some(enrichment);
            }
        }
        Ok(())
    }
}
