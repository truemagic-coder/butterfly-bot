use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct LearningProfile {
    pub strategy: String,
    pub success: bool,
}

pub struct MetaLearningBrain {
    last_profile: Mutex<Option<LearningProfile>>,
}

impl MetaLearningBrain {
    pub fn new() -> Self {
        Self {
            last_profile: Mutex::new(None),
        }
    }

    pub async fn last_profile(&self) -> Option<LearningProfile> {
        let guard = self.last_profile.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> LearningProfile {
        let lower = message.to_lowercase();
        if lower.contains("example") {
            return LearningProfile {
                strategy: "example_driven".to_string(),
                success: true,
            };
        }
        if lower.contains("confused") || lower.contains("not sure") {
            return LearningProfile {
                strategy: "theory_first".to_string(),
                success: false,
            };
        }
        LearningProfile {
            strategy: "incremental".to_string(),
            success: true,
        }
    }
}

#[async_trait]
impl BrainPlugin for MetaLearningBrain {
    fn name(&self) -> &str {
        "meta_learning"
    }

    fn description(&self) -> &str {
        "Tracks which teaching strategies work"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let profile = Self::assess(&text);
            let mut guard = self.last_profile.lock().await;
            *guard = Some(profile);
        }
        Ok(())
    }
}
