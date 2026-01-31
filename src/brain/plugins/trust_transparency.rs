use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

const DATA_COLLECTION: &str = "We keep conversation data locally or in the configured store so the assistant can remember context. We avoid collecting unnecessary personal data.";
const DATA_USAGE: &str =
    "Data is used to provide your session and memory. It is not sold or used for advertising.";
const DATA_DELETION: &str = "You can delete your conversation history at any time; local data is removed when you clear it.";
const SECURITY: &str =
    "We use local-first defaults and standard transport encryption for network calls.";

pub struct TrustTransparencyBrain {
    last_response: Mutex<Option<String>>,
}

impl TrustTransparencyBrain {
    pub fn new() -> Self {
        Self {
            last_response: Mutex::new(None),
        }
    }

    pub async fn last_response(&self) -> Option<String> {
        let guard = self.last_response.lock().await;
        guard.clone()
    }

    fn classify(message: &str) -> Option<&'static str> {
        let lower = message.to_lowercase();
        if lower.contains("data") && lower.contains("collect") {
            return Some(DATA_COLLECTION);
        }
        if lower.contains("data") && (lower.contains("use") || lower.contains("train")) {
            return Some(DATA_USAGE);
        }
        if lower.contains("delete") || lower.contains("remove my data") {
            return Some(DATA_DELETION);
        }
        if lower.contains("secure") || lower.contains("encryption") {
            return Some(SECURITY);
        }
        None
    }
}

#[async_trait]
impl BrainPlugin for TrustTransparencyBrain {
    fn name(&self) -> &str {
        "trust_transparency"
    }

    fn description(&self) -> &str {
        "Explains privacy and trust practices when asked"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if let Some(response) = Self::classify(&text) {
                let mut guard = self.last_response.lock().await;
                *guard = Some(response.to_string());
            }
        }
        Ok(())
    }
}
