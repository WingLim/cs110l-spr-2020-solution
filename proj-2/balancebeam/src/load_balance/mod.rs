use std::sync::Arc;
use async_trait::async_trait;
use crate::ProxyState;
use self::{random::Random, round_robin::RoundRobin};

pub mod random;
pub mod round_robin;

#[derive(clap::ArgEnum, Debug)]
pub enum ArgLoadBalance {
    Random,
    RoundRobin
}

#[async_trait]
pub trait LoadBalanceStrategy: Send + Sync {
    async fn select_backend<'l>(&'l self, state: &'l Arc<ProxyState>) -> Option<usize>;
}

impl From<ArgLoadBalance> for Box<dyn LoadBalanceStrategy> {
    fn from(other: ArgLoadBalance) -> Self {
        match other {
            ArgLoadBalance::Random => {
                Box::new(Random::new())
            }
            ArgLoadBalance::RoundRobin => {
                Box::new(RoundRobin::new())
            }
        }
    }
}
