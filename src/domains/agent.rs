use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AIAgent {
    pub name: String,
    pub instructions: String,
    pub specialization: String,
    pub capture_name: Option<String>,
    pub capture_schema: Option<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct BusinessMission {
    pub mission: Option<String>,
    pub voice: Option<String>,
    pub values: Vec<(String, String)>,
    pub goals: Vec<String>,
}
