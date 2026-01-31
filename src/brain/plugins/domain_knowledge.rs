use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DomainKnowledgeSignal {
    pub domain: String,
    pub confidence: f32,
}

pub struct DomainKnowledgeBrain {
    last_signal: Mutex<Option<DomainKnowledgeSignal>>,
}

impl DomainKnowledgeBrain {
    pub fn new() -> Self {
        Self {
            last_signal: Mutex::new(None),
        }
    }

    pub async fn last_signal(&self) -> Option<DomainKnowledgeSignal> {
        let guard = self.last_signal.lock().await;
        guard.clone()
    }

    fn detect_domain(message: &str) -> DomainKnowledgeSignal {
        let lower = message.to_lowercase();
        let domain = if lower.contains("finance") || lower.contains("budget") {
            "financial"
        } else if lower.contains("career") || lower.contains("job") {
            "career"
        } else if lower.contains("health") || lower.contains("sleep") {
            "wellness"
        } else {
            "general"
        };
        DomainKnowledgeSignal {
            domain: domain.to_string(),
            confidence: 0.55,
        }
    }
}

#[async_trait]
impl BrainPlugin for DomainKnowledgeBrain {
    fn name(&self) -> &str {
        "domain_knowledge"
    }

    fn description(&self) -> &str {
        "Tags the domain for lightweight routing"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let signal = Self::detect_domain(&text);
            let mut guard = self.last_signal.lock().await;
            *guard = Some(signal);
        }
        Ok(())
    }
}
