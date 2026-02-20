use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

pub struct ContextAwarenessBrain {
    recent_topics: Mutex<HashMap<String, Vec<String>>>,
    last_hint: Mutex<Option<String>>,
}

impl ContextAwarenessBrain {
    pub fn new() -> Self {
        Self {
            recent_topics: Mutex::new(HashMap::new()),
            last_hint: Mutex::new(None),
        }
    }

    pub async fn last_hint(&self) -> Option<String> {
        let guard = self.last_hint.lock().await;
        guard.clone()
    }

    fn extract_topics(text: &str) -> Vec<String> {
        let mut topics = Vec::new();
        let keywords = [
            "project",
            "meeting",
            "trip",
            "vacation",
            "job",
            "interview",
            "exam",
            "presentation",
            "dog",
            "cat",
            "relationship",
            "startup",
            "promotion",
        ];
        let lower = text.to_lowercase();
        for keyword in &keywords {
            if lower.contains(keyword) {
                topics.push(keyword.to_string());
            }
        }
        let words: Vec<&str> = lower.split_whitespace().collect();
        for idx in 0..words.len().saturating_sub(1) {
            if words[idx] == "my" || words[idx] == "our" {
                let next = words[idx + 1];
                if next.len() > 3 {
                    topics.push(next.to_string());
                }
            }
        }
        topics
    }

    fn detect_pronoun(text: &str) -> bool {
        let pronouns = ["it", "this", "that", "they", "them", "he", "she"];
        text.to_lowercase()
            .split_whitespace()
            .any(|word| pronouns.contains(&word.trim_matches(|c: char| !c.is_alphanumeric())))
    }
}

#[async_trait]
impl BrainPlugin for ContextAwarenessBrain {
    fn name(&self) -> &str {
        "context_awareness"
    }

    fn description(&self) -> &str {
        "Tracks recent topics and offers lightweight context hints"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let user_id = ctx.user_id.clone().unwrap_or_default();
            let topics = Self::extract_topics(&text);
            let mut recent = self.recent_topics.lock().await;
            let entry = recent.entry(user_id.clone()).or_default();
            for topic in topics {
                if !entry.contains(&topic) {
                    entry.insert(0, topic);
                }
            }
            entry.truncate(10);

            if Self::detect_pronoun(&text) {
                if let Some(topic) = entry.first() {
                    let hint = format!("User likely refers to {topic}");
                    let mut guard = self.last_hint.lock().await;
                    *guard = Some(hint);
                }
            }
        }
        Ok(())
    }
}
