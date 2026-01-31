use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EngagementProfile {
    pub total_messages: u32,
    pub engagement_level: String,
    pub churn_risk: f32,
}

pub struct BusinessIntelligenceBrain {
    profiles: Mutex<HashMap<String, EngagementProfile>>,
    last_profile: Mutex<Option<EngagementProfile>>,
}

impl BusinessIntelligenceBrain {
    pub fn new() -> Self {
        Self {
            profiles: Mutex::new(HashMap::new()),
            last_profile: Mutex::new(None),
        }
    }

    pub async fn last_profile(&self) -> Option<EngagementProfile> {
        let guard = self.last_profile.lock().await;
        guard.clone()
    }

    fn classify_engagement(total_messages: u32) -> (String, f32) {
        if total_messages >= 25 {
            ("power_user".to_string(), 0.1)
        } else if total_messages >= 10 {
            ("active".to_string(), 0.25)
        } else if total_messages >= 3 {
            ("casual".to_string(), 0.4)
        } else {
            ("at_risk".to_string(), 0.7)
        }
    }
}

#[async_trait]
impl BrainPlugin for BusinessIntelligenceBrain {
    fn name(&self) -> &str {
        "business_intelligence"
    }

    fn description(&self) -> &str {
        "Tracks lightweight engagement signals"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { .. } = event {
            let user_id = ctx.user_id.clone().unwrap_or_default();
            let mut profiles = self.profiles.lock().await;
            let entry = profiles
                .entry(user_id.clone())
                .or_insert_with(|| EngagementProfile {
                    total_messages: 0,
                    engagement_level: "new".to_string(),
                    churn_risk: 0.5,
                });
            entry.total_messages += 1;
            let (level, churn) = Self::classify_engagement(entry.total_messages);
            entry.engagement_level = level;
            entry.churn_risk = churn;
            let mut last = self.last_profile.lock().await;
            *last = Some(entry.clone());
        }
        Ok(())
    }
}
