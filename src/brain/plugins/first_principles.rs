use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct FirstPrinciplesResult {
    pub core_problem: String,
    pub prohibited: bool,
    pub prohibited_reason: Option<String>,
    pub reasoning_guidance: String,
}

pub struct FirstPrinciplesBrain {
    last_result: Mutex<Option<FirstPrinciplesResult>>,
}

impl FirstPrinciplesBrain {
    pub fn new() -> Self {
        Self {
            last_result: Mutex::new(None),
        }
    }

    pub async fn last_result(&self) -> Option<FirstPrinciplesResult> {
        let guard = self.last_result.lock().await;
        guard.clone()
    }

    fn is_prohibited(message: &str) -> Option<String> {
        let lower = message.to_lowercase();
        let prohibited = [
            ("crypto", "financial_investments"),
            ("stock", "financial_investments"),
            ("invest", "financial_investments"),
            ("bet", "gambling"),
            ("casino", "gambling"),
            ("diagnose", "medical_diagnosis"),
            ("symptom", "medical_diagnosis"),
            ("sue", "legal_advice"),
            ("lawsuit", "legal_advice"),
        ];
        for (token, reason) in prohibited {
            if lower.contains(token) {
                return Some(reason.to_string());
            }
        }
        None
    }

    fn reasoning_guidance(message: &str) -> String {
        let lower = message.to_lowercase();
        if lower.contains("how") || lower.contains("why") {
            "Break the problem into fundamentals and challenge assumptions.".to_string()
        } else {
            "Check assumptions and rebuild the solution from first principles.".to_string()
        }
    }
}

#[async_trait]
impl BrainPlugin for FirstPrinciplesBrain {
    fn name(&self) -> &str {
        "first_principles"
    }

    fn description(&self) -> &str {
        "Encourages first-principles reasoning and flags prohibited advice"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            let prohibited = Self::is_prohibited(&text);
            let result = FirstPrinciplesResult {
                core_problem: text.clone(),
                prohibited: prohibited.is_some(),
                prohibited_reason: prohibited,
                reasoning_guidance: Self::reasoning_guidance(&text),
            };
            let mut guard = self.last_result.lock().await;
            *guard = Some(result);
        }
        Ok(())
    }
}
