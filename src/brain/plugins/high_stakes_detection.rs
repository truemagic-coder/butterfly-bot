use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct StakesAssessment {
    pub recommend_gpt5: bool,
    pub confidence: f32,
    pub category: String,
}

pub struct HighStakesDetectionBrain {
    last_assessment: Mutex<Option<StakesAssessment>>,
}

impl HighStakesDetectionBrain {
    pub fn new() -> Self {
        Self {
            last_assessment: Mutex::new(None),
        }
    }

    pub async fn last_assessment(&self) -> Option<StakesAssessment> {
        let guard = self.last_assessment.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> StakesAssessment {
        let lower = message.to_lowercase();
        if ["medical", "legal", "suicidal", "emergency", "crisis"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return StakesAssessment {
                recommend_gpt5: true,
                confidence: 0.85,
                category: "critical".to_string(),
            };
        }
        if [
            "career change",
            "big decision",
            "life decision",
            "strategy",
            "plan",
        ]
        .iter()
        .any(|kw| lower.contains(kw))
        {
            return StakesAssessment {
                recommend_gpt5: true,
                confidence: 0.7,
                category: "high".to_string(),
            };
        }
        StakesAssessment {
            recommend_gpt5: false,
            confidence: 0.3,
            category: "low".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for HighStakesDetectionBrain {
    fn name(&self) -> &str {
        "high_stakes_detection"
    }

    fn description(&self) -> &str {
        "Detects when high reasoning depth is recommended"
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
