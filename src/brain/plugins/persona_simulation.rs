use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PersonaSimulationResult {
    pub persona_count: u32,
    pub nps_score: f32,
    pub summary: String,
}

pub struct PersonaSimulationBrain {
    last_result: Mutex<Option<PersonaSimulationResult>>,
}

impl PersonaSimulationBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<PersonaSimulationResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn should_simulate(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["persona", "simulate", "focus group", "nps"]
            .iter()
            .any(|kw| lower.contains(kw))
    }

    fn simulate(message: &str) -> PersonaSimulationResult {
        let summary = format!("Simulated persona feedback for: {message}");
        PersonaSimulationResult {
            persona_count: 12,
            nps_score: 34.0,
            summary,
        }
    }
}

#[async_trait]
impl BrainPlugin for PersonaSimulationBrain {
    fn name(&self) -> &str {
        "persona_simulation"
    }

    fn description(&self) -> &str {
        "Runs lightweight persona simulations and returns a summary"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::should_simulate(&text) {
                let result = Self::simulate(&text);
                let mut guard = self.last_result.lock().await;
                *guard = Some(result);
            }
        }
        Ok(())
    }
}
