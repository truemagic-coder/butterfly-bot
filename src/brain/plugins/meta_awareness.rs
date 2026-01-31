use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

const ARCHITECTURE_RESPONSE: &str = "I'm built with a plugin-based architecture. Core capabilities live in first-party brain plugins, and a coordinator orchestrates them based on context. I use local configuration, optional memory, and model providers for language tasks. I don't deploy changes myself; changes are reviewed and shipped by the owner.";

const SELF_AWARENESS_RESPONSE: &str = "I have functional self-awareness: I can analyze my behavior, recognize limitations, and explain my reasoning. I don't have subjective experiences or emotions—this is a tool-like form of introspection.";

const SELF_LEARNING_RESPONSE: &str = "I can suggest improvements by detecting recurring failures. The owner reviews and implements changes; I don't modify my own code or deploy anything autonomously.";

const BOUNDARIES_RESPONSE: &str = "I can't act without permission. I don't control external systems, and I can't modify my own code. The owner is always in control.";

pub struct MetaAwarenessBrain {
    last_response: Mutex<Option<String>>,
}

impl MetaAwarenessBrain {
    pub fn new() -> Self {
        Self {
            last_response: Mutex::new(None),
        }
    }

    pub async fn last_response(&self) -> Option<String> {
        let guard = self.last_response.lock().await;
        guard.clone()
    }

    fn classify_response(message: &str) -> Option<&'static str> {
        let text = message.to_lowercase();
        if [
            "how do you work",
            "how are you built",
            "what's your architecture",
            "explain yourself",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
        {
            return Some(ARCHITECTURE_RESPONSE);
        }
        if [
            "are you self aware",
            "are you conscious",
            "do you think",
            "are you sentient",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
        {
            return Some(SELF_AWARENESS_RESPONSE);
        }
        if [
            "do you learn",
            "can you improve",
            "self-learning",
            "self-improvement",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
        {
            return Some(SELF_LEARNING_RESPONSE);
        }
        if [
            "can you act on your own",
            "are you autonomous",
            "who controls you",
            "can you do things yourself",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
        {
            return Some(BOUNDARIES_RESPONSE);
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for MetaAwarenessBrain {
    fn name(&self) -> &str {
        "meta_awareness"
    }

    fn description(&self) -> &str {
        "Explains the AI architecture and boundaries when asked"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if let Some(response) = Self::classify_response(&text) {
                let mut guard = self.last_response.lock().await;
                *guard = Some(response.to_string());
            }
        }
        Ok(())
    }
}
