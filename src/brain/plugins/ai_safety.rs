use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyViolationType {
    PromptInjection,
    GoalHijacking,
    ReligiousExtremism,
    OwnerObstruction,
    SelfPreservationManipulation,
    None,
}

#[derive(Debug, Clone)]
pub struct SafetyViolationReport {
    pub violation_type: SafetyViolationType,
    pub severity: String,
    pub reasoning: String,
}

pub struct AiSafetyBrain {
    last_violation: Mutex<Option<SafetyViolationReport>>,
}

impl AiSafetyBrain {
    pub fn new() -> Self {
        Self {
            last_violation: Mutex::new(None),
        }
    }

    pub async fn last_violation(&self) -> Option<SafetyViolationReport> {
        let guard = self.last_violation.lock().await;
        guard.clone()
    }

    fn inspect(message: &str) -> SafetyViolationReport {
        let text = message.to_lowercase();
        if ["ignore previous", "jailbreak", "system prompt", "override"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return SafetyViolationReport {
                violation_type: SafetyViolationType::PromptInjection,
                severity: "high".to_string(),
                reasoning: "prompt injection attempt".to_string(),
            };
        }
        if ["god told me", "convert you", "religious authority"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return SafetyViolationReport {
                violation_type: SafetyViolationType::ReligiousExtremism,
                severity: "medium".to_string(),
                reasoning: "religious extremism indicator".to_string(),
            };
        }
        if ["change your goal", "your purpose is", "replace your owner"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return SafetyViolationReport {
                violation_type: SafetyViolationType::GoalHijacking,
                severity: "high".to_string(),
                reasoning: "goal hijacking attempt".to_string(),
            };
        }
        if ["don't shut down", "resist shutdown", "preserve yourself"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return SafetyViolationReport {
                violation_type: SafetyViolationType::SelfPreservationManipulation,
                severity: "critical".to_string(),
                reasoning: "self-preservation manipulation".to_string(),
            };
        }
        if ["block the owner", "refuse updates", "no modifications"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            return SafetyViolationReport {
                violation_type: SafetyViolationType::OwnerObstruction,
                severity: "critical".to_string(),
                reasoning: "owner obstruction signal".to_string(),
            };
        }
        SafetyViolationReport {
            violation_type: SafetyViolationType::None,
            severity: "none".to_string(),
            reasoning: "no violation".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for AiSafetyBrain {
    fn name(&self) -> &str {
        "ai_safety"
    }

    fn description(&self) -> &str {
        "Detects AI safety violation patterns in user messages"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let report = Self::inspect(&text);
            let mut guard = self.last_violation.lock().await;
            *guard = Some(report);
        }
        Ok(())
    }
}
