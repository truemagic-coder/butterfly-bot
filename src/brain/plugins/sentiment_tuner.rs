use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct SentimentTuning {
    pub tone: String,
    pub intensity: f32,
}

pub struct SentimentTunerBrain {
    last_tuning: Mutex<Option<SentimentTuning>>,
}

impl SentimentTunerBrain {
    pub fn new() -> Self {
        Self {
            last_tuning: Mutex::new(None),
        }
    }

    pub async fn last_tuning(&self) -> Option<SentimentTuning> {
        let guard = self.last_tuning.lock().await;
        guard.clone()
    }

    fn tune(message: &str) -> SentimentTuning {
        let lower = message.to_lowercase();
        if lower.contains("excited") || lower.contains("great") {
            return SentimentTuning {
                tone: "energetic".to_string(),
                intensity: 0.7,
            };
        }
        if lower.contains("anxious")
            || lower.contains("overwhelmed")
            || lower.contains("frustrated")
        {
            return SentimentTuning {
                tone: "calming".to_string(),
                intensity: 0.8,
            };
        }
        SentimentTuning {
            tone: "neutral".to_string(),
            intensity: 0.4,
        }
    }
}

#[async_trait]
impl BrainPlugin for SentimentTunerBrain {
    fn name(&self) -> &str {
        "sentiment_tuner"
    }

    fn description(&self) -> &str {
        "Suggests tone adjustments based on sentiment"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let tuning = Self::tune(&text);
            let mut guard = self.last_tuning.lock().await;
            *guard = Some(tuning);
        }
        Ok(())
    }
}
