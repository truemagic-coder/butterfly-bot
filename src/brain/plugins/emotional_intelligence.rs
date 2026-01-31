use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EmotionalDetection {
    pub primary_emotion: String,
    pub intensity: String,
    pub recommended_style: String,
}

pub struct EmotionalIntelligenceBrain {
    last_detection: Mutex<Option<EmotionalDetection>>,
}

impl EmotionalIntelligenceBrain {
    pub fn new() -> Self {
        Self {
            last_detection: Mutex::new(None),
        }
    }

    pub async fn last_detection(&self) -> Option<EmotionalDetection> {
        let guard = self.last_detection.lock().await;
        guard.clone()
    }

    fn detect(message: &str) -> EmotionalDetection {
        let lower = message.to_lowercase();
        if lower.contains("overwhelmed") || lower.contains("stressed") {
            return EmotionalDetection {
                primary_emotion: "overwhelmed".to_string(),
                intensity: "moderate".to_string(),
                recommended_style: "calm_soothing".to_string(),
            };
        }
        if lower.contains("sad") || lower.contains("down") {
            return EmotionalDetection {
                primary_emotion: "sad".to_string(),
                intensity: "moderate".to_string(),
                recommended_style: "empathetic".to_string(),
            };
        }
        EmotionalDetection {
            primary_emotion: "neutral".to_string(),
            intensity: "mild".to_string(),
            recommended_style: "supportive".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for EmotionalIntelligenceBrain {
    fn name(&self) -> &str {
        "emotional_intelligence"
    }

    fn description(&self) -> &str {
        "Detects user emotion and recommends a response style"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let detection = Self::detect(&text);
            let mut guard = self.last_detection.lock().await;
            *guard = Some(detection);
        }
        Ok(())
    }
}
