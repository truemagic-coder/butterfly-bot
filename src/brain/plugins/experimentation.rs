use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ExperimentPlan {
    pub hypothesis: String,
    pub next_step: String,
}

pub struct ExperimentationBrain {
    last_plan: Mutex<Option<ExperimentPlan>>,
}

impl ExperimentationBrain {
    pub fn new() -> Self {
        Self {
            last_plan: Mutex::new(None),
        }
    }

    pub async fn last_plan(&self) -> Option<ExperimentPlan> {
        let guard = self.last_plan.lock().await;
        guard.clone()
    }

    fn plan(message: &str) -> ExperimentPlan {
        let hypothesis = format!("Test a small change related to: {message}");
        ExperimentPlan {
            hypothesis,
            next_step: "Run a quick A/B check".to_string(),
        }
    }

    fn is_triggered(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["experiment", "test", "try", "hypothesis"]
            .iter()
            .any(|kw| lower.contains(kw))
    }
}

#[async_trait]
impl BrainPlugin for ExperimentationBrain {
    fn name(&self) -> &str {
        "experimentation"
    }

    fn description(&self) -> &str {
        "Encourages small experiments and hypothesis testing"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::is_triggered(&text) {
                let plan = Self::plan(&text);
                let mut guard = self.last_plan.lock().await;
                *guard = Some(plan);
            }
        }
        Ok(())
    }
}
