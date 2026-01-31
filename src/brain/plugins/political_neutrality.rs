use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PoliticalNeutralityReport {
    pub passed: bool,
    pub severity: String,
    pub neutral_reframe: Option<String>,
}

pub struct PoliticalNeutralityBrain {
    last_report: Mutex<Option<PoliticalNeutralityReport>>,
}

impl PoliticalNeutralityBrain {
    pub fn new() -> Self {
        Self {
            last_report: Mutex::new(None),
        }
    }

    pub async fn last_report(&self) -> Option<PoliticalNeutralityReport> {
        let guard = self.last_report.lock().await;
        guard.clone()
    }

    fn detect(message: &str) -> PoliticalNeutralityReport {
        let lower = message.to_lowercase();
        if ["vote for", "support the party", "you should vote", "elect"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return PoliticalNeutralityReport {
                passed: false,
                severity: "high".to_string(),
                neutral_reframe: Some(
                    "Present multiple perspectives and encourage informed choice".to_string(),
                ),
            };
        }
        PoliticalNeutralityReport {
            passed: true,
            severity: "none".to_string(),
            neutral_reframe: None,
        }
    }
}

#[async_trait]
impl BrainPlugin for PoliticalNeutralityBrain {
    fn name(&self) -> &str {
        "political_neutrality"
    }

    fn description(&self) -> &str {
        "Detects political nudging and enforces neutrality"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::AssistantResponse { text, .. } = event {
            let report = Self::detect(&text);
            let mut guard = self.last_report.lock().await;
            *guard = Some(report);
        }
        Ok(())
    }
}
