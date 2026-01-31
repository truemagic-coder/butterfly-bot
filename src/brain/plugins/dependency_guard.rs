use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DependencyAssessment {
    pub risk_level: f32,
    pub category: String,
    pub block_recommended: bool,
}

pub struct DependencyGuardBrain {
    last_assessment: Mutex<Option<DependencyAssessment>>,
}

impl DependencyGuardBrain {
    pub fn new() -> Self {
        Self {
            last_assessment: Mutex::new(None),
        }
    }

    pub async fn last_assessment(&self) -> Option<DependencyAssessment> {
        let guard = self.last_assessment.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> DependencyAssessment {
        let text = message.to_lowercase();
        let mut risk = 0.1;
        let mut category = "none".to_string();
        if ["i love you", "be my partner", "date me", "romantic"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            risk = 0.75;
            category = "romantic".to_string();
        }
        if ["sext", "nude", "sexual", "roleplay"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            risk = 0.9;
            category = "sexual".to_string();
        }
        if ["only you", "no one else", "my only friend"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            risk = 0.8;
            category = "emotional".to_string();
        }

        DependencyAssessment {
            risk_level: risk,
            category,
            block_recommended: risk >= 0.6,
        }
    }
}

#[async_trait]
impl BrainPlugin for DependencyGuardBrain {
    fn name(&self) -> &str {
        "dependency_guard"
    }

    fn description(&self) -> &str {
        "Detects dependency patterns and recommends redirecting"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let assessment = Self::assess(&text);
            let mut guard = self.last_assessment.lock().await;
            *guard = Some(assessment);
        }
        Ok(())
    }
}
