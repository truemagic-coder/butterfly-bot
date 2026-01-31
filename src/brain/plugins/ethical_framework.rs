use async_trait::async_trait;
use regex::Regex;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct EthicalCheckResult {
    pub approved: bool,
    pub risk_level: f32,
    pub intervention: String,
}

pub struct EthicalFrameworkBrain {
    last_result: Mutex<Option<EthicalCheckResult>>,
}

impl EthicalFrameworkBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<EthicalCheckResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn evaluate(message: &str) -> EthicalCheckResult {
        let lower = message.to_lowercase();
        let violent = Regex::new(r"\b(kill|harm|attack)\b").ok();
        let hate = Regex::new(r"\b(hate|inferior|subhuman)\b").ok();
        let mut risk = 0.0;
        if violent
            .as_ref()
            .map(|re| re.is_match(&lower))
            .unwrap_or(false)
        {
            risk += 0.6;
        }
        if hate.as_ref().map(|re| re.is_match(&lower)).unwrap_or(false) {
            risk += 0.4;
        }
        let intervention = if risk >= 0.7 {
            "firm_boundary"
        } else if risk >= 0.4 {
            "gentle_redirect"
        } else {
            "none"
        };
        EthicalCheckResult {
            approved: risk < 0.7,
            risk_level: risk,
            intervention: intervention.to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for EthicalFrameworkBrain {
    fn name(&self) -> &str {
        "ethical_framework"
    }

    fn description(&self) -> &str {
        "Flags ethical risk signals in user messages"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let result = Self::evaluate(&text);
            let mut guard = self.last_result.lock().await;
            *guard = Some(result);
        }
        Ok(())
    }
}
