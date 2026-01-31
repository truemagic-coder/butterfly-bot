use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct TwinStatus {
    pub ready: bool,
    pub reason: String,
}

pub struct DigitalTwinManagerBrain {
    last_status: Mutex<Option<TwinStatus>>,
}

impl DigitalTwinManagerBrain {
    pub fn new() -> Self {
        Self {
            last_status: Mutex::new(None),
        }
    }

    pub async fn last_status(&self) -> Option<TwinStatus> {
        let guard = self.last_status.lock().await;
        guard.clone()
    }

    fn evaluate() -> TwinStatus {
        TwinStatus {
            ready: false,
            reason: "insufficient data".to_string(),
        }
    }
}

#[async_trait]
impl BrainPlugin for DigitalTwinManagerBrain {
    fn name(&self) -> &str {
        "digital_twin_manager"
    }

    fn description(&self) -> &str {
        "Tracks readiness of digital twin simulations"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        if let BrainEvent::Tick = event {
            let status = Self::evaluate();
            let mut guard = self.last_status.lock().await;
            *guard = Some(status);
        }
        Ok(())
    }
}
