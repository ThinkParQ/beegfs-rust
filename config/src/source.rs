use crate::*;
use async_trait::async_trait;

#[async_trait]
pub trait Source {
    async fn get(&self) -> Result<ConfigMap, BoxedError>;
}
