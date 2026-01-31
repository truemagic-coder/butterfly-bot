use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct NeedSuggestion {
    pub limitation: String,
    pub evidence_count: u32,
    pub title: String,
}

pub struct NeedRecognitionBrain {
    counts: Mutex<HashMap<String, u32>>,
    last_suggestion: Mutex<Option<NeedSuggestion>>,
    threshold: u32,
}

impl NeedRecognitionBrain {
    pub fn new() -> Self {
        Self {
            counts: Mutex::new(HashMap::new()),
            last_suggestion: Mutex::new(None),
            threshold: 3,
        }
    }

    pub async fn last_suggestion(&self) -> Option<NeedSuggestion> {
        let guard = self.last_suggestion.lock().await;
        guard.clone()
    }

    fn detect_limitation(message: &str) -> Option<String> {
        let lower = message.to_lowercase();
        if ["forgot", "don't remember", "didn't remember"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return Some("memory".to_string());
        }
        if ["not what i meant", "misunderstood", "wrong"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return Some("understanding".to_string());
        }
        if ["slow", "takes too long"]
            .iter()
            .any(|kw| lower.contains(kw))
        {
            return Some("performance".to_string());
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for NeedRecognitionBrain {
    fn name(&self) -> &str {
        "need_recognition"
    }

    fn description(&self) -> &str {
        "Detects recurring limitations and suggests improvements"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let Some(limitation) = Self::detect_limitation(&text) else {
                return Ok(());
            };

            let mut counts = self.counts.lock().await;
            let count = counts.entry(limitation.clone()).or_insert(0);
            *count += 1;

            if *count >= self.threshold {
                let suggestion = NeedSuggestion {
                    limitation: limitation.clone(),
                    evidence_count: *count,
                    title: format!("Improve {} handling", limitation),
                };
                let mut guard = self.last_suggestion.lock().await;
                *guard = Some(suggestion);
            }
        }
        Ok(())
    }
}
