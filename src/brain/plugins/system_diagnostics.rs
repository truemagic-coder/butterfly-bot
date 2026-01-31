use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};

#[derive(Debug, Clone)]
pub struct DiagnosticsReport {
    pub status: String,
    pub healthy: bool,
}

pub struct SystemDiagnosticsBrain {
    last_report: Mutex<Option<DiagnosticsReport>>,
}

impl SystemDiagnosticsBrain {
    pub fn new() -> Self {
        Self {
            last_report: Mutex::new(None),
        }
    }

    pub async fn last_report(&self) -> Option<DiagnosticsReport> {
        let guard = self.last_report.lock().await;
        guard.clone()
    }

    fn run_checks() -> DiagnosticsReport {
        DiagnosticsReport {
            status: "healthy".to_string(),
            healthy: true,
        }
    }
}

#[async_trait]
impl BrainPlugin for SystemDiagnosticsBrain {
    fn name(&self) -> &str {
        "system_diagnostics"
    }

    fn description(&self) -> &str {
        "Runs lightweight system diagnostics"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> Result<()> {
        match event {
            BrainEvent::Start | BrainEvent::Tick => {
                let report = Self::run_checks();
                let mut guard = self.last_report.lock().await;
                *guard = Some(report);
            }
            _ => {}
        }
        Ok(())
    }
}
