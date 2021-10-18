use std::net::IpAddr;

pub mod counter;

#[derive(clap::ArgEnum, Debug)]
pub enum ArgRateLimiter {
    Counter
}

pub trait RateLimiterStrategy: Send + Sync {
    fn register_request(&mut self, addr: IpAddr) -> bool;

    fn refresh(&mut self);
}
