use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct PlanStep {
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct DeepPlan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

pub struct DeepPlanningBrain {
    last_plan: Mutex<Option<DeepPlan>>,
}

impl DeepPlanningBrain {
    pub fn new() -> Self {
        Self {
            last_plan: Mutex::new(None),
        }
    }

    pub async fn last_plan(&self) -> Option<DeepPlan> {
        let guard = self.last_plan.lock().await;
        guard.clone()
    }

    fn needs_planning(message: &str) -> bool {
        let lower = message.to_lowercase();
        ["plan", "goal", "roadmap", "strategy", "steps", "transition"]
            .iter()
            .any(|kw| lower.contains(kw))
    }

    fn create_plan(message: &str) -> DeepPlan {
        let goal = message.trim().to_string();
        let steps = vec![
            PlanStep {
                description: "Clarify the goal and constraints".to_string(),
            },
            PlanStep {
                description: "Break into milestones".to_string(),
            },
            PlanStep {
                description: "Execute next concrete step".to_string(),
            },
        ];
        DeepPlan { goal, steps }
    }
}

#[async_trait]
impl BrainPlugin for DeepPlanningBrain {
    fn name(&self) -> &str {
        "deep_planning"
    }

    fn description(&self) -> &str {
        "Creates a lightweight multi-step plan"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if Self::needs_planning(&text) {
                let plan = Self::create_plan(&text);
                let mut guard = self.last_plan.lock().await;
                *guard = Some(plan);
            }
        }
        Ok(())
    }
}
