use std::sync::Arc;
use async_trait::async_trait;
use crate::ProxyState;

pub mod random;
pub mod round_robin;

#[async_trait]
pub trait LoadBalancingStrategy: Send + Sync {
    async fn select_backend<'l>(&'l self, state: &'l Arc<ProxyState>) -> Option<usize>;
}