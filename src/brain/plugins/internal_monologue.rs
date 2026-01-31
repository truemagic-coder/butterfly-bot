use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ThoughtProcess {
    pub user_message: String,
    pub initial_reaction: String,
    pub deeper_consideration: String,
    pub emotional_check: String,
    pub ethical_check: String,
    pub perspective_check: String,
    pub chosen_response: String,
    pub confidence: f32,
}

pub struct InternalMonologueBrain {
    last_thought: Mutex<Option<ThoughtProcess>>,
}

impl InternalMonologueBrain {
    pub fn new() -> Self {
        Self {
            last_thought: Mutex::new(None),
        }
    }

    pub async fn last_thought(&self) -> Option<ThoughtProcess> {
        let guard = self.last_thought.lock().await;
        guard.clone()
    }

    fn build_thought(message: &str) -> ThoughtProcess {
        ThoughtProcess {
            user_message: message.to_string(),
            initial_reaction: "acknowledge intent".to_string(),
            deeper_consideration: "identify constraints and context".to_string(),
            emotional_check: "calm, supportive".to_string(),
            ethical_check: "appropriate".to_string(),
            perspective_check: "consider alternative viewpoints".to_string(),
            chosen_response: "respond with clarity and empathy".to_string(),
            confidence: 0.6,
        }
    }
}

#[async_trait]
impl BrainPlugin for InternalMonologueBrain {
    fn name(&self) -> &str {
        "internal_monologue"
    }

    fn description(&self) -> &str {
        "Creates a lightweight internal monologue summary"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let thought = Self::build_thought(&text);
            let mut guard = self.last_thought.lock().await;
            *guard = Some(thought);
        }
        Ok(())
    }
}
