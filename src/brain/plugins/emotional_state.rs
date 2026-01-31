use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EmotionalStateSnapshot {
    pub emotions: HashMap<String, f32>,
    pub baseline_mood: f32,
    pub current_mood: f32,
}

pub struct EmotionalStateBrain {
    last_state: Mutex<Option<EmotionalStateSnapshot>>,
}

impl EmotionalStateBrain {
    pub fn new() -> Self {
        Self {
            last_state: Mutex::new(None),
        }
    }

    pub async fn last_state(&self) -> Option<EmotionalStateSnapshot> {
        let guard = self.last_state.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> EmotionalStateSnapshot {
        let lower = message.to_lowercase();
        let mut emotions = HashMap::new();
        if lower.contains("excited") || lower.contains("awesome") {
            emotions.insert("excitement".to_string(), 0.7);
        }
        if lower.contains("worried") || lower.contains("anxious") {
            emotions.insert("concern".to_string(), 0.6);
        }
        if lower.contains("confused") || lower.contains("unsure") {
            emotions.insert("uncertainty".to_string(), 0.5);
        }
        let current_mood = if emotions.is_empty() { 0.5 } else { 0.55 };
        EmotionalStateSnapshot {
            emotions,
            baseline_mood: 0.5,
            current_mood,
        }
    }
}

#[async_trait]
impl BrainPlugin for EmotionalStateBrain {
    fn name(&self) -> &str {
        "emotional_state"
    }

    fn description(&self) -> &str {
        "Tracks a lightweight emotional state snapshot"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let snapshot = Self::analyze(&text);
            let mut guard = self.last_state.lock().await;
            *guard = Some(snapshot);
        }
        Ok(())
    }
}
