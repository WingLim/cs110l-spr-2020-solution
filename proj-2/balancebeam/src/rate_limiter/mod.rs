use std::net::IpAddr;

pub mod counter;

pub trait RateLimiterStrategy: Send + Sync {
    fn register_request(&mut self, addr: IpAddr) -> bool;

    fn refresh(&mut self);
}