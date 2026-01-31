use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct RelationalInsight {
    pub insight: String,
    pub confidence: f32,
}

pub struct RelationalInsightBrain {
    last_insight: Mutex<Option<RelationalInsight>>,
}

impl RelationalInsightBrain {
    pub fn new() -> Self {
        Self {
            last_insight: Mutex::new(None),
        }
    }

    pub async fn last_insight(&self) -> Option<RelationalInsight> {
        let guard = self.last_insight.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> RelationalInsight {
        let lower = message.to_lowercase();
        let insight = if lower.contains("argument") || lower.contains("fight") {
            "Tension detected; suggest calm framing".to_string()
        } else if lower.contains("family") || lower.contains("partner") {
            "Relationship context detected".to_string()
        } else {
            "General relational context".to_string()
        };
        RelationalInsight {
            insight,
            confidence: 0.6,
        }
    }
}

#[async_trait]
impl BrainPlugin for RelationalInsightBrain {
    fn name(&self) -> &str {
        "relational_insight"
    }

    fn description(&self) -> &str {
        "Extracts lightweight relational insights"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let insight = Self::analyze(&text);
            let mut guard = self.last_insight.lock().await;
            *guard = Some(insight);
        }
        Ok(())
    }
}
