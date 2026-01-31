use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DeepInsight {
    pub category: String,
    pub description: String,
    pub confidence: f32,
}

pub struct DeepInsightBrain {
    last_insight: Mutex<Option<DeepInsight>>,
}

impl DeepInsightBrain {
    pub fn new() -> Self {
        Self {
            last_insight: Mutex::new(None),
        }
    }

    pub async fn last_insight(&self) -> Option<DeepInsight> {
        let guard = self.last_insight.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> DeepInsight {
        let lower = message.to_lowercase();
        if lower.contains("want to") || lower.contains("goal") || lower.contains("plan") {
            return DeepInsight {
                category: "goal".to_string(),
                description: format!("User is articulating a goal: {message}"),
                confidence: 0.75,
            };
        }
        if ["stressed", "anxious", "excited", "sad", "overwhelmed"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return DeepInsight {
                category: "emotion".to_string(),
                description: "User is expressing a strong emotional state".to_string(),
                confidence: 0.7,
            };
        }
        DeepInsight {
            category: "pattern".to_string(),
            description: "User shared a recurring theme".to_string(),
            confidence: 0.55,
        }
    }
}

#[async_trait]
impl BrainPlugin for DeepInsightBrain {
    fn name(&self) -> &str {
        "deep_insight"
    }

    fn description(&self) -> &str {
        "Generates lightweight deep insights from user messages"
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
