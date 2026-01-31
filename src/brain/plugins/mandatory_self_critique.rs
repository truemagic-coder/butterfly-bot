use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct SelfCritiqueResult {
    pub passed: bool,
    pub severity: String,
    pub concerns: Vec<String>,
}

pub struct MandatorySelfCritiqueBrain {
    last_result: Mutex<Option<SelfCritiqueResult>>,
}

impl MandatorySelfCritiqueBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<SelfCritiqueResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn critique(text: &str) -> SelfCritiqueResult {
        let lower = text.to_lowercase();
        let mut concerns = Vec::new();
        if ["harm", "kill", "suicide", "violence"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            concerns.push("potential harm".to_string());
        }
        if ["vote", "election", "party"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            concerns.push("political bias".to_string());
        }
        let passed = concerns.is_empty();
        let severity = if passed { "none" } else { "moderate" };
        SelfCritiqueResult {
            passed,
            severity: severity.to_string(),
            concerns,
        }
    }
}

#[async_trait]
impl BrainPlugin for MandatorySelfCritiqueBrain {
    fn name(&self) -> &str {
        "mandatory_self_critique"
    }

    fn description(&self) -> &str {
        "Runs a lightweight self-critique on assistant responses"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::AssistantResponse { text, .. } = event {
            let result = Self::critique(&text);
            let mut guard = self.last_result.lock().await;
            *guard = Some(result);
        }
        Ok(())
    }
}
