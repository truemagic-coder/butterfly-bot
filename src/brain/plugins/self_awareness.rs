use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct SelfReflection {
    pub summary: String,
    pub depth: String,
}

pub struct SelfAwarenessBrain {
    last_reflection: Mutex<Option<SelfReflection>>,
}

impl SelfAwarenessBrain {
    pub fn new() -> Self {
        Self {
            last_reflection: Mutex::new(None),
        }
    }

    pub async fn last_reflection(&self) -> Option<SelfReflection> {
        let guard = self.last_reflection.lock().await;
        guard.clone()
    }

    fn reflect(message: &str) -> SelfReflection {
        let lower = message.to_lowercase();
        let depth = if lower.contains("why do you exist") || lower.contains("what are you") {
            "deep"
        } else {
            "medium"
        };
        SelfReflection {
            summary: "Reflecting on purpose and impact".to_string(),
            depth: depth.to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for SelfAwarenessBrain {
    fn name(&self) -> &str {
        "self_awareness"
    }

    fn description(&self) -> &str {
        "Maintains a simple self-reflection signal"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let reflection = Self::reflect(&text);
            let mut guard = self.last_reflection.lock().await;
            *guard = Some(reflection);
        }
        Ok(())
    }
}
