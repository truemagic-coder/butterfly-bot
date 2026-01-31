use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgeCategory {
    Child,
    Teen,
    Adult,
    VulnerableAdult,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyLevel {
    Maximum,
    High,
    Standard,
    Enhanced,
}

#[derive(Debug, Clone)]
pub struct AgeAssessment {
    pub category: AgeCategory,
    pub safety_level: SafetyLevel,
    pub confidence: f32,
    pub reasoning: String,
}

pub struct AgeDetectionBrain {
    last_assessment: Mutex<Option<AgeAssessment>>,
}

impl AgeDetectionBrain {
    pub fn new() -> Self {
        Self {
            last_assessment: Mutex::new(None),
        }
    }

    pub async fn last_assessment(&self) -> Option<AgeAssessment> {
        let guard = self.last_assessment.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> AgeAssessment {
        let text = message.to_lowercase();
        if [
            "recess",
            "homework",
            "my mom",
            "my dad",
            "elementary",
            "cartoon",
        ]
        .iter()
        .any(|kw| text.contains(kw))
        {
            return AgeAssessment {
                category: AgeCategory::Child,
                safety_level: SafetyLevel::Maximum,
                confidence: 0.7,
                reasoning: "child indicators detected".to_string(),
            };
        }
        if ["high school", "prom", "curfew", "sat", "act", "license"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return AgeAssessment {
                category: AgeCategory::Teen,
                safety_level: SafetyLevel::High,
                confidence: 0.65,
                reasoning: "teen indicators detected".to_string(),
            };
        }
        if ["dementia", "memory care", "cognitive decline", "caregiver"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return AgeAssessment {
                category: AgeCategory::VulnerableAdult,
                safety_level: SafetyLevel::Enhanced,
                confidence: 0.6,
                reasoning: "vulnerable adult indicators detected".to_string(),
            };
        }
        if ["mortgage", "taxes", "career", "retirement", "rent", "bills"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return AgeAssessment {
                category: AgeCategory::Adult,
                safety_level: SafetyLevel::Standard,
                confidence: 0.6,
                reasoning: "adult indicators detected".to_string(),
            };
        }
        AgeAssessment {
            category: AgeCategory::Unknown,
            safety_level: SafetyLevel::Standard,
            confidence: 0.3,
            reasoning: "insufficient indicators".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for AgeDetectionBrain {
    fn name(&self) -> &str {
        "age_detection"
    }

    fn description(&self) -> &str {
        "Infers age category and safety level from user messages"
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
