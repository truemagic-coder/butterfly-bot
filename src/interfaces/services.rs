use async_trait::async_trait;

use crate::error::Result;

#[async_trait]
pub trait RoutingService: Send + Sync {
    async fn route_query(&self, query: &str) -> Result<String>;
}
