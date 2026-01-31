use async_trait::async_trait;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct BrainContext {
    pub agent_name: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BrainEvent {
    Start,
    Tick,
    UserMessage { user_id: String, text: String },
    AssistantResponse { user_id: String, text: String },
}

#[async_trait]
pub trait BrainPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn on_event(&self, event: BrainEvent, ctx: &BrainContext) -> Result<()>;
}
