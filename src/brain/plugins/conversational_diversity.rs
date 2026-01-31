use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DiversityAnalysis {
    pub is_stale: bool,
    pub topic: Option<String>,
    pub overused_topics: Vec<String>,
    pub suggested_topics: Vec<String>,
}

pub struct ConversationalDiversityBrain {
    last_topic: Mutex<HashMap<String, String>>,
    repeat_count: Mutex<HashMap<String, u32>>,
    topic_counts: Mutex<HashMap<String, HashMap<String, u32>>>,
    last_analysis: Mutex<Option<DiversityAnalysis>>,
}

impl ConversationalDiversityBrain {
    pub fn new() -> Self {
        Self {
            last_topic: Mutex::new(HashMap::new()),
            repeat_count: Mutex::new(HashMap::new()),
            topic_counts: Mutex::new(HashMap::new()),
            last_analysis: Mutex::new(None),
        }
    }

    pub async fn last_analysis(&self) -> Option<DiversityAnalysis> {
        let guard = self.last_analysis.lock().await;
        guard.clone()
    }

    fn detect_topic(text: &str) -> Option<String> {
        let lower = text.to_lowercase();
        let keywords = [
            "work",
            "project",
            "travel",
            "health",
            "fitness",
            "food",
            "music",
            "career",
            "relationship",
            "study",
        ];
        for keyword in &keywords {
            if lower.contains(keyword) {
                return Some(keyword.to_string());
            }
        }
        let words: Vec<&str> = lower.split_whitespace().collect();
        for idx in 0..words.len().saturating_sub(1) {
            if words[idx] == "my" {
                let next = words[idx + 1];
                if next.len() > 3 {
                    return Some(next.to_string());
                }
            }
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for ConversationalDiversityBrain {
    fn name(&self) -> &str {
        "conversational_diversity"
    }

    fn description(&self) -> &str {
        "Detects stale topics and suggests fresh directions"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let user_id = ctx.user_id.clone().unwrap_or_default();
            let topic = Self::detect_topic(&text);
            if topic.is_none() {
                return Ok(());
            }
            let topic = topic.unwrap();

            let mut last_topic = self.last_topic.lock().await;
            let mut repeat_count = self.repeat_count.lock().await;
            let mut topic_counts = self.topic_counts.lock().await;

            let count_entry = topic_counts
                .entry(user_id.clone())
                .or_insert_with(HashMap::new);
            *count_entry.entry(topic.clone()).or_insert(0) += 1;

            let repeats = if last_topic.get(&user_id) == Some(&topic) {
                let next = repeat_count.entry(user_id.clone()).or_insert(1);
                *next += 1;
                *next
            } else {
                repeat_count.insert(user_id.clone(), 1);
                last_topic.insert(user_id.clone(), topic.clone());
                1
            };

            let overused_topics: Vec<String> = count_entry
                .iter()
                .filter_map(|(topic, count)| {
                    if *count >= 3 {
                        Some(topic.clone())
                    } else {
                        None
                    }
                })
                .collect();

            let is_stale = repeats >= 3;
            let mut suggested_topics = vec![
                "hobbies".to_string(),
                "travel".to_string(),
                "learning".to_string(),
                "wellbeing".to_string(),
                "career".to_string(),
            ];
            suggested_topics.retain(|item| item != &topic);

            let analysis = DiversityAnalysis {
                is_stale,
                topic: Some(topic),
                overused_topics,
                suggested_topics,
            };
            let mut guard = self.last_analysis.lock().await;
            *guard = Some(analysis);
        }
        Ok(())
    }
}
