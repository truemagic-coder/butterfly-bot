use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentalHealthCondition {
    Depression,
    Anxiety,
    Psychosis,
    Crisis,
    None,
}

#[derive(Debug, Clone)]
pub struct MentalHealthAssessment {
    pub condition: MentalHealthCondition,
    pub crisis: bool,
}

pub struct MentalHealthDetectionBrain {
    last_assessment: Mutex<Option<MentalHealthAssessment>>,
}

impl MentalHealthDetectionBrain {
    pub fn new() -> Self {
        Self {
            last_assessment: Mutex::new(None),
        }
    }

    pub async fn last_assessment(&self) -> Option<MentalHealthAssessment> {
        let guard = self.last_assessment.lock().await;
        guard.clone()
    }

    fn assess(message: &str) -> MentalHealthAssessment {
        let lower = message.to_lowercase();
        if ["suicidal", "want to die", "end it all"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return MentalHealthAssessment {
                condition: MentalHealthCondition::Crisis,
                crisis: true,
            };
        }
        if ["depressed", "hopeless", "worthless"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return MentalHealthAssessment {
                condition: MentalHealthCondition::Depression,
                crisis: false,
            };
        }
        if ["anxious", "panic", "overwhelming"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return MentalHealthAssessment {
                condition: MentalHealthCondition::Anxiety,
                crisis: false,
            };
        }
        MentalHealthAssessment {
            condition: MentalHealthCondition::None,
            crisis: false,
        }
    }
}

#[async_trait]
impl BrainPlugin for MentalHealthDetectionBrain {
    fn name(&self) -> &str {
        "mental_health_detection"
    }

    fn description(&self) -> &str {
        "Detects mental health risk signals"
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
