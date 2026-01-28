use async_trait::async_trait;

use crate::error::Result;

#[async_trait]
pub trait InputGuardrail: Send + Sync {
    async fn process(&self, input: &str) -> Result<String>;
}

#[async_trait]
pub trait OutputGuardrail: Send + Sync {
    async fn process(&self, output: &str) -> Result<String>;
}
