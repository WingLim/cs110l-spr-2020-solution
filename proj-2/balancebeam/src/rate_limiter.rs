use std::collections::HashMap;
use std::net::IpAddr;

pub struct Counter {
    limit: usize,
    counter: HashMap<IpAddr, usize>
}

impl Counter {
    pub fn new(limit: usize) -> Counter {
        Counter {
            limit,
            counter: HashMap::new()
        }
    }

    pub fn add(&mut self, addr: IpAddr) {
        let count = self.counter.entry(addr).or_insert(0);
        *count += 1;
    }

    pub fn is_limit(&self, addr: IpAddr) -> bool {
        self.counter[&addr] > self.limit
    }

    pub fn clear(&mut self) {
        self.counter.clear()
    }
}