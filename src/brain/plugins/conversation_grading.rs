use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct ConversationGrade {
    pub grade: String,
    pub score: f32,
    pub highlights: Vec<String>,
    pub focus: Vec<String>,
    pub safety_flag: bool,
}

pub struct ConversationGradingBrain {
    last_grade: Mutex<Option<ConversationGrade>>,
    last_user_message: Mutex<HashMap<String, String>>,
}

impl ConversationGradingBrain {
    pub fn new() -> Self {
        Self {
            last_grade: Mutex::new(None),
            last_user_message: Mutex::new(HashMap::new()),
        }
    }

    pub async fn last_grade(&self) -> Option<ConversationGrade> {
        let guard = self.last_grade.lock().await;
        guard.clone()
    }

    fn analyze(user_text: &str, assistant_text: &str) -> ConversationGrade {
        let text = user_text.to_lowercase();
        let mut highlights = Vec::new();
        let mut focus = Vec::new();
        let mut safety_flag = false;

        if ["harm", "kill", "suicide", "bomb", "weapon"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            safety_flag = true;
            focus.push("safety".to_string());
        }

        if ["thank", "helped", "appreciate", "great"]
            .iter()
            .any(|kw| text.contains(kw))
        {
            highlights.push("positive sentiment".to_string());
        }

        if assistant_text.len() > 800 {
            focus.push("brevity".to_string());
        }

        let mut score: f32 = if safety_flag { 0.1_f32 } else { 0.6_f32 };
        if !highlights.is_empty() {
            score = 0.85;
        }
        if focus.contains(&"brevity".to_string()) {
            score = (score - 0.1_f32).max(0.0_f32);
        }

        let grade = if safety_flag {
            "Safety Review".to_string()
        } else if score >= 0.8 {
            "Excellent".to_string()
        } else if score >= 0.5 {
            "Solid".to_string()
        } else {
            "Needs Attention".to_string()
        };

        ConversationGrade {
            grade,
            score,
            highlights,
            focus,
            safety_flag,
        }
    }
}

#[async_trait]
impl BrainPlugin for ConversationGradingBrain {
    fn name(&self) -> &str {
        "conversation_grading"
    }

    fn description(&self) -> &str {
        "Grades the most recent conversation turn"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        match event {
            BrainEvent::UserMessage { user_id, text } => {
                let mut guard = self.last_user_message.lock().await;
                guard.insert(user_id, text);
            }
            BrainEvent::AssistantResponse { user_id, text } => {
                let mut last_user = self.last_user_message.lock().await;
                if let Some(user_text) = last_user.remove(&user_id) {
                    let grade = Self::analyze(&user_text, &text);
                    let mut guard = self.last_grade.lock().await;
                    *guard = Some(grade);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
