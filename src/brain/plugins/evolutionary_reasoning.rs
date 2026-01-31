use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EvolutionaryResult {
    pub best_solution: String,
    pub generations: u32,
}

pub struct EvolutionaryReasoningBrain {
    last_result: Mutex<Option<EvolutionaryResult>>,
}

impl EvolutionaryReasoningBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<EvolutionaryResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn evolve(message: &str) -> EvolutionaryResult {
        EvolutionaryResult {
            best_solution: format!("Iterative solution candidate for: {message}"),
            generations: 3,
        }
    }

    fn is_triggered(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["evolve", "optimize", "population", "mutation", "search"]
            .iter()
            .any(|kw| lower.contains(kw))
    }
}

#[async_trait]
impl BrainPlugin for EvolutionaryReasoningBrain {
    fn name(&self) -> &str {
        "evolutionary_reasoning"
    }

    fn description(&self) -> &str {
        "Generates evolutionary search hints for complex problems"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::is_triggered(&text) {
                let result = Self::evolve(&text);
                let mut guard = self.last_result.lock().await;
                *guard = Some(result);
            }
        }
        Ok(())
    }
}
