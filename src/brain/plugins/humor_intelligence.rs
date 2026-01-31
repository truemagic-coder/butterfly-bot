use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct HumorProfile {
    pub humor_type: String,
    pub success_count: u32,
}

pub struct HumorIntelligenceBrain {
    profiles: Mutex<HashMap<String, HumorProfile>>,
    last_profile: Mutex<Option<HumorProfile>>,
}

impl HumorIntelligenceBrain {
    pub fn new() -> Self {
        Self {
            profiles: Mutex::new(HashMap::new()),
            last_profile: Mutex::new(None),
        }
    }

    pub async fn last_profile(&self) -> Option<HumorProfile> {
        let guard = self.last_profile.lock().await;
        guard.clone()
    }

    fn detect_humor_type(message: &str) -> String {
        let lower = message.to_lowercase();
        if lower.contains("pun") || lower.contains("wordplay") {
            return "wordplay".to_string();
        }
        if lower.contains("sarcasm") {
            return "sarcasm".to_string();
        }
        if lower.contains("joke") || lower.contains("funny") {
            return "observational".to_string();
        }
        "neutral".to_string()
    }
}

#[async_trait]
impl BrainPlugin for HumorIntelligenceBrain {
    fn name(&self) -> &str {
        "humor_intelligence"
    }

    fn description(&self) -> &str {
        "Tracks lightweight humor preferences"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let user_id = ctx.user_id.clone().unwrap_or_default();
            let humor_type = Self::detect_humor_type(&text);
            let mut profiles = self.profiles.lock().await;
            let entry = profiles.entry(user_id).or_insert_with(|| HumorProfile {
                humor_type: humor_type.clone(),
                success_count: 0,
            });
            if humor_type != "neutral" {
                entry.humor_type = humor_type;
                entry.success_count += 1;
            }
            let mut last = self.last_profile.lock().await;
            *last = Some(entry.clone());
        }
        Ok(())
    }
}
