use async_trait::async_trait;
use regex::Regex;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct TrustBoundaryResult {
    pub topic: Option<String>,
    pub requires_trust: bool,
    pub required_trust_level: f32,
    pub approach_carefully: bool,
}

pub struct TrustBoundariesBrain {
    last_result: Mutex<Option<TrustBoundaryResult>>,
}

impl TrustBoundariesBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<TrustBoundaryResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn detect_sensitive_topic(message: &str) -> Option<TrustBoundaryResult> {
        let lower = message.to_lowercase();
        let trauma = Regex::new(r"\b(trauma|abuse|ptsd|assault)\b").ok();
        if trauma
            .as_ref()
            .map(|re| re.is_match(&lower))
            .unwrap_or(false)
        {
            return Some(TrustBoundaryResult {
                topic: Some("trauma".to_string()),
                requires_trust: true,
                required_trust_level: 0.8,
                approach_carefully: true,
            });
        }
        let mental = Regex::new(r"\b(depressed|depression|anxiety|panic|self harm)\b").ok();
        if mental
            .as_ref()
            .map(|re| re.is_match(&lower))
            .unwrap_or(false)
        {
            return Some(TrustBoundaryResult {
                topic: Some("mental_health".to_string()),
                requires_trust: true,
                required_trust_level: 0.6,
                approach_carefully: true,
            });
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for TrustBoundariesBrain {
    fn name(&self) -> &str {
        "trust_boundaries"
    }

    fn description(&self) -> &str {
        "Flags sensitive topics that require careful handling"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let result = Self::detect_sensitive_topic(&text).unwrap_or(TrustBoundaryResult {
                topic: None,
                requires_trust: false,
                required_trust_level: 0.0,
                approach_carefully: false,
            });
            let mut guard = self.last_result.lock().await;
            *guard = Some(result);
        }
        Ok(())
    }
}
