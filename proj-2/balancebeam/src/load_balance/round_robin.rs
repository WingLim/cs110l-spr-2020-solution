use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use crate::ProxyState;
use super::LoadBalanceStrategy;

pub struct RoundRobin {
    rrc: Arc<Mutex<u32>>
}

impl RoundRobin {
    pub fn new() -> RoundRobin {
        RoundRobin{
            rrc: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LoadBalanceStrategy for RoundRobin {
    async fn select_backend<'l>(&'l self, state: &'l Arc<ProxyState>) -> Option<usize> {
        let upstream_status = state.upstream_status.read().await;
        if upstream_status.all_dead() {
            return None;
        }

        let mut rrc_handle = self.rrc.lock().unwrap();

        loop {
            *rrc_handle = (*rrc_handle + 1) % state.upstream_addresses.len() as u32;
            let idx = *rrc_handle as usize;
            if upstream_status.is_alive(idx) {
                return Some(idx);
            }
        }
        
    }
}