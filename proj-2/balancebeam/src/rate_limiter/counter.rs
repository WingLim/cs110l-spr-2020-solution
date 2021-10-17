use std::collections::HashMap;
use std::net::IpAddr;
use super::RateLimiterStrategy;

pub struct Counter {
    limit: usize,
    requests: HashMap<IpAddr, usize>
}

impl Counter {
    pub fn new(limit: usize) -> Counter {
        Counter {
            limit,
            requests: HashMap::new()
        }
    }
}

impl RateLimiterStrategy for Counter {
    fn register_request(&mut self, addr: IpAddr) -> bool {
        let count = self.requests.entry(addr).or_insert(0);
        *count += 1;
        *count <= self.limit
    }

    fn refresh(&mut self) {
        self.requests.clear()
    }
}