use std::collections::HashMap;
use std::net::IpAddr;

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

    pub fn register_request(&mut self, addr: IpAddr) -> bool {
        let count = self.requests.entry(addr).or_insert(0);
        *count += 1;
        *count <= self.limit
    }

    pub fn clear(&mut self) {
        self.requests.clear()
    }
}