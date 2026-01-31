use async_trait::async_trait;
use regex::Regex;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct CriticalThinkingAnalysis {
    pub fallacies_detected: Vec<String>,
    pub argument_strength: String,
    pub critical_questions: Vec<String>,
    pub should_gently_correct: bool,
}

pub struct CriticalThinkingBrain {
    last_analysis: Mutex<Option<CriticalThinkingAnalysis>>,
}

impl CriticalThinkingBrain {
    pub fn new() -> Self {
        Self {
            last_analysis: Mutex::new(None),
        }
    }

    pub async fn last_analysis(&self) -> Option<CriticalThinkingAnalysis> {
        let guard = self.last_analysis.lock().await;
        guard.clone()
    }

    fn contains_argument(message: &str) -> bool {
        let indicators = [
            "because",
            "therefore",
            "thus",
            "since",
            "should",
            "must",
            "proves",
            "shows",
            "clearly",
            "obviously",
            "everyone knows",
            "studies show",
            "research says",
        ];
        let lower = message.to_lowercase();
        indicators.iter().any(|item| lower.contains(item))
    }

    fn detect_fallacies(message: &str) -> Vec<String> {
        let lower = message.to_lowercase();
        let mut fallacies = Vec::new();

        if lower.contains("everyone knows") {
            fallacies.push("appeal_to_popularity".to_string());
        }
        if lower.contains("because i said so") {
            fallacies.push("appeal_to_authority".to_string());
        }
        if lower.contains("if you don't") && lower.contains("then") {
            fallacies.push("false_dichotomy".to_string());
        }
        if lower.contains("you are") {
            let ad_hominem = Regex::new(r"you are (stupid|dumb|ignorant)").ok();
            if ad_hominem
                .as_ref()
                .map(|re| re.is_match(&lower))
                .unwrap_or(false)
            {
                fallacies.push("ad_hominem".to_string());
            }
        }

        fallacies
    }

    fn critical_questions(message: &str) -> Vec<String> {
        let mut questions = Vec::new();
        if message.to_lowercase().contains("should") {
            questions.push("What evidence supports this recommendation?".to_string());
        }
        questions.push("What assumptions are required for this to be true?".to_string());
        questions.push("Are there credible counterexamples?".to_string());
        questions
    }
}

#[async_trait]
impl BrainPlugin for CriticalThinkingBrain {
    fn name(&self) -> &str {
        "critical_thinking"
    }

    fn description(&self) -> &str {
        "Flags potential reasoning issues and suggests critical questions"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::UserMessage { text, .. } = event {
            if !Self::contains_argument(&text) {
                return Ok(());
            }
            let fallacies = Self::detect_fallacies(&text);
            let questions = Self::critical_questions(&text);
            let should_correct = !fallacies.is_empty();
            let strength = if should_correct { "moderate" } else { "strong" };
            let analysis = CriticalThinkingAnalysis {
                fallacies_detected: fallacies,
                argument_strength: strength.to_string(),
                critical_questions: questions,
                should_gently_correct: should_correct,
            };
            let mut guard = self.last_analysis.lock().await;
            *guard = Some(analysis);
        }
        Ok(())
    }
}
