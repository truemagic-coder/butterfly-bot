use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct GroundingSnapshot {
    pub anchors: Vec<String>,
    pub diversity_score: f32,
    pub nudge: Option<String>,
}

pub struct GroundingBrain {
    user_anchors: Mutex<HashMap<String, Vec<String>>>,
    last_snapshot: Mutex<Option<GroundingSnapshot>>,
}

impl GroundingBrain {
    pub fn new() -> Self {
        Self {
            user_anchors: Mutex::new(HashMap::new()),
            last_snapshot: Mutex::new(None),
        }
    }

    pub async fn last_snapshot(&self) -> Option<GroundingSnapshot> {
        let guard = self.last_snapshot.lock().await;
        guard.clone()
    }

    fn extract_anchors(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let keywords = [
            "family",
            "kids",
            "spouse",
            "job",
            "hobby",
            "faith",
            "culture",
            "community",
        ];
        keywords
            .iter()
            .filter(|kw| lower.contains(*kw))
            .map(|kw| kw.to_string())
            .collect()
    }
}

#[async_trait]
impl BrainPlugin for GroundingBrain {
    fn name(&self) -> &str {
        "grounding"
    }

    fn description(&self) -> &str {
        "Tracks identity anchors and nudges against over-assimilation"
    }

    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let user_id = ctx.user_id.clone().unwrap_or_default();
            let anchors = Self::extract_anchors(&text);
            let mut stored = self.user_anchors.lock().await;
            let entry = stored.entry(user_id.clone()).or_default();
            for anchor in anchors.iter() {
                if !entry.contains(anchor) {
                    entry.push(anchor.clone());
                }
            }
            let diversity_score = (entry.len() as f32 / 8.0).min(1.0);
            let nudge = if text.to_lowercase().contains("just like you") {
                Some("Encourage user to reflect on their own values".to_string())
            } else {
                None
            };
            let snapshot = GroundingSnapshot {
                anchors: entry.clone(),
                diversity_score,
                nudge,
            };
            let mut guard = self.last_snapshot.lock().await;
            *guard = Some(snapshot);
        }
        Ok(())
    }
}
