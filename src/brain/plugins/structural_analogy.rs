use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct StructuralAnalogyResult {
    pub source_domain: String,
    pub target_domain: String,
    pub insight: String,
}

pub struct StructuralAnalogyBrain {
    last_result: Mutex<Option<StructuralAnalogyResult>>,
}

impl StructuralAnalogyBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<StructuralAnalogyResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn analyze(message: &str) -> StructuralAnalogyResult {
        let lower = message.to_lowercase();
        let (source, target) = if lower.contains("code") || lower.contains("software") {
            ("technical", "personal_growth")
        } else {
            ("personal_growth", "technical")
        };
        StructuralAnalogyResult {
            source_domain: source.to_string(),
            target_domain: target.to_string(),
            insight: "Look for shared structure across domains".to_string(),
        }
    }

    fn should_analyze(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["problem", "stuck", "challenge", "analogy"]
            .iter()
            .any(|kw| lower.contains(kw))
    }
}

#[async_trait]
impl BrainPlugin for StructuralAnalogyBrain {
    fn name(&self) -> &str {
        "structural_analogy"
    }

    fn description(&self) -> &str {
        "Detects structural analogies across domains"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::should_analyze(&text) {
                let result = Self::analyze(&text);
                let mut guard = self.last_result.lock().await;
                *guard = Some(result);
            }
        }
        Ok(())
    }
}
