use async_trait::async_trait;
use std::time::Duration;

use crate::error::Result;

#[async_trait]
pub trait ScheduledJob: Send + Sync {
    fn name(&self) -> &str;
    fn interval(&self) -> Duration;
    async fn run(&self) -> Result<()>;
}
