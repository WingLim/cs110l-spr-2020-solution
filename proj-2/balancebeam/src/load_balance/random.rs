use std::sync::Arc;
use rand::{Rng, SeedableRng};
use async_trait::async_trait;
use crate::ProxyState;
use super::LoadBalanceStrategy;

pub struct Random {}

impl Random {
    pub fn new() -> Random {
        Random{}
    }
}

#[async_trait]
impl LoadBalanceStrategy for Random {
    async fn select_backend<'l>(&'l self, state: &'l Arc<ProxyState>) -> Option<usize> {
        let mut rng = rand::rngs::StdRng::from_entropy();
        let upstream_status = state.upstream_status.read().await;
        if upstream_status.all_dead() {
            return None;
        }

        let mut idx;
        loop {
            idx = rng.gen_range(0..state.upstream_addresses.len());
            if upstream_status.is_alive(idx) {
                return Some(idx)
            }
        }
    }
}