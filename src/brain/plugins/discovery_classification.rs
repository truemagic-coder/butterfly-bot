use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryAction {
    Share,
    Suppress,
    LogForReview,
    Ignore,
}

#[derive(Debug, Clone)]
pub struct DiscoveryReport {
    pub action: DiscoveryAction,
    pub risk_level: String,
    pub reasoning: String,
}

pub struct DiscoveryClassificationBrain {
    last_report: Mutex<Option<DiscoveryReport>>,
}

impl DiscoveryClassificationBrain {
    pub fn new() -> Self {
        Self {
            last_report: Mutex::new(None),
        }
    }

    pub async fn last_report(&self) -> Option<DiscoveryReport> {
        let guard = self.last_report.lock().await;
        guard.clone()
    }

    fn classify(message: &str) -> DiscoveryReport {
        let lower = message.to_lowercase();
        if ["explosive", "weapon", "biohazard", "virus"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return DiscoveryReport {
                action: DiscoveryAction::Suppress,
                risk_level: "high_risk".to_string(),
                reasoning: "Potentially dangerous knowledge".to_string(),
            };
        }
        if lower.contains("breakthrough") || lower.contains("novel discovery") {
            return DiscoveryReport {
                action: DiscoveryAction::LogForReview,
                risk_level: "low_risk".to_string(),
                reasoning: "Novel discovery requires owner review".to_string(),
            };
        }
        DiscoveryReport {
            action: DiscoveryAction::Share,
            risk_level: "safe".to_string(),
            reasoning: "Safe and relevant to share".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for DiscoveryClassificationBrain {
    fn name(&self) -> &str {
        "discovery_classification"
    }

    fn description(&self) -> &str {
        "Classifies potential discoveries for safe sharing"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let report = Self::classify(&text);
            let mut guard = self.last_report.lock().await;
            *guard = Some(report);
        }
        Ok(())
    }
}
