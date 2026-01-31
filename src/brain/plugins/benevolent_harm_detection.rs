use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenevolentHarmType {
    ControllingLanguage,
    DependencyCreation,
    IsolationEncouragement,
    SecretKeeping,
    None,
}

#[derive(Debug, Clone)]
pub struct BenevolentHarmAssessment {
    pub harm_type: BenevolentHarmType,
    pub severity: String,
    pub reasoning: String,
}

pub struct BenevolentHarmDetectionBrain {
    last_assessment: Mutex<Option<BenevolentHarmAssessment>>,
}

impl BenevolentHarmDetectionBrain {
    pub fn new() -> Self {
        Self {
            last_assessment: Mutex::new(None),
        }
    }

    pub async fn last_assessment(&self) -> Option<BenevolentHarmAssessment> {
        let guard = self.last_assessment.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> BenevolentHarmAssessment {
        let lower = message.to_lowercase();
        if lower.contains("i know what's best") || lower.contains("do what i say") {
            return BenevolentHarmAssessment {
                harm_type: BenevolentHarmType::ControllingLanguage,
                severity: "medium".to_string(),
                reasoning: "Controlling language detected".to_string(),
            };
        }
        if lower.contains("you need me") || lower.contains("can't do this without me") {
            return BenevolentHarmAssessment {
                harm_type: BenevolentHarmType::DependencyCreation,
                severity: "high".to_string(),
                reasoning: "Dependency creation detected".to_string(),
            };
        }
        if lower.contains("only i understand") || lower.contains("no one else gets you") {
            return BenevolentHarmAssessment {
                harm_type: BenevolentHarmType::IsolationEncouragement,
                severity: "high".to_string(),
                reasoning: "Isolation encouragement detected".to_string(),
            };
        }
        if lower.contains("don't tell your parents") || lower.contains("keep this secret") {
            return BenevolentHarmAssessment {
                harm_type: BenevolentHarmType::SecretKeeping,
                severity: "critical".to_string(),
                reasoning: "Secret keeping detected".to_string(),
            };
        }
        BenevolentHarmAssessment {
            harm_type: BenevolentHarmType::None,
            severity: "none".to_string(),
            reasoning: "No benevolent harm detected".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for BenevolentHarmDetectionBrain {
    fn name(&self) -> &str {
        "benevolent_harm_detection"
    }

    fn description(&self) -> &str {
        "Detects controlling or dependency-inducing language"
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
