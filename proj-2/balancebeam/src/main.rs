mod request;
mod response;
mod rate_limiter;
mod load_balance;

use std::{io::ErrorKind, sync::Arc};
use clap::Clap;
use tokio::{net::{TcpListener, TcpStream}, sync::{Mutex, RwLock}, time::{sleep, Duration}};
use crate::rate_limiter::counter::Counter;
use crate::rate_limiter::{RateLimiterStrategy, ArgRateLimiter};
use crate::load_balance::{LoadBalanceStrategy, ArgLoadBalance};

/// Contains information parsed from the command-line invocation of balancebeam. The Clap macros
/// provide a fancy way to automatically construct a command-line argument parser.
#[derive(Clap, Debug)]
#[clap(about = "Fun with load balancing")]
struct CmdOptions {
    #[clap(
        short,
        long,
        about = "IP/port to bind to",
        default_value = "0.0.0.0:1100"
    )]
    bind: String,
    #[clap(short, long, multiple_occurrences = true, about = "Upstream host to forward requests to")]
    upstream: Vec<String>,
    #[clap(
        long,
        about = "Perform active health checks on this interval (in seconds)",
        default_value = "10"
    )]
    active_health_check_interval: usize,
    #[clap(
    long,
    about = "Path to send request to for active health checks",
    default_value = "/"
    )]
    active_health_check_path: String,
    #[clap(
        long,
        about = "Maximum number of requests to accept per IP per minute (0 = unlimited)",
        default_value = "0"
    )]
    max_requests_per_minute: usize,
    #[clap(
        arg_enum,
        long,
        about = "Rate limit strategy",
        default_value = "counter",
    )]
    rate_limiter: ArgRateLimiter,
    #[clap(
        arg_enum,
        long,
        about = "Load balance strategy",
        default_value = "round-robin",
    )]
    load_balancer: ArgLoadBalance,
}

/// Contains information about the state of balancebeam (e.g. what servers we are currently proxying
/// to, what servers have failed, rate limiting counts, etc.)
///
/// You should add fields to this struct in later milestones.
pub struct ProxyState {
    /// How frequently we check whether upstream servers are alive (Milestone 4)
    active_health_check_interval: usize,
    /// Where we should send requests when doing active health checks (Milestone 4)
    active_health_check_path: String,
    /// Maximum number of requests an individual IP can make in a minute (Milestone 5)
    #[allow(dead_code)]
    max_requests_per_minute: usize,
    /// Addresses of servers that we are proxying to
    upstream_addresses: Vec<String>,
    /// Status of upstream servers
    upstream_status: RwLock<UpstreamsStatus>,
    /// Strategy of limiter to use
    limiter: Mutex<Box<dyn RateLimiterStrategy>>,
    /// Strategy of load balancer to use
    load_balancer: Box<dyn LoadBalanceStrategy>
}

struct UpstreamsStatus {
    /// Alive upstream counts
    counts: usize,
    /// Upstream status, one by one match upstream_addresses
    status: Vec<bool>
}

impl UpstreamsStatus {
    fn new(counts: usize) -> UpstreamsStatus {
        UpstreamsStatus { 
            counts, 
            status: vec![true; counts]
        }
    }

    fn is_alive(&self, idx: usize) -> bool {
        self.status[idx]
    }

    fn all_dead(&self) -> bool {
        self.counts == 0
    }

    fn set_up(&mut self, idx: usize) {
        if !self.is_alive(idx) { 
            self.counts += 1;
            self.status[idx] = true;
        }
    }

    fn set_down(&mut self, idx: usize) {
        if self.is_alive(idx) {
            self.counts -= 1;
            self.status[idx] = false;
        }
    }
}

#[tokio::main]
async fn main() {
    // Initialize the logging library. You can print log messages using the `log` macros:
    // https://docs.rs/log/0.4.8/log/ You are welcome to continue using print! statements; this
    // just looks a little prettier.
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::init();

    // Parse the command line arguments passed to this program
    let options = CmdOptions::parse();
    if options.upstream.len() < 1 {
        log::error!("At least one upstream server must be specified using the --upstream option.");
        std::process::exit(1);
    }

    // Start listening for connections
    let listener = match TcpListener::bind(&options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    let upstreams_counts = options.upstream.len();
    // Handle incoming connections
    let state = ProxyState {
        upstream_addresses: options.upstream,
        upstream_status: RwLock::new(UpstreamsStatus::new(upstreams_counts)),
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
        limiter: Mutex::new(set_up_rate_limiter(options.rate_limiter, options.max_requests_per_minute)),
        load_balancer: options.load_balancer.into(),
    };
    
    let shared_state = Arc::new(state);
    
    let shared_state_ref = shared_state.clone();
    tokio::spawn(async move {
        active_health_check(shared_state_ref).await;
    });

    if shared_state.max_requests_per_minute > 0 {
        let shared_state_ref = shared_state.clone();
        tokio::spawn(async move {
            limiter_refresh(shared_state_ref, 60).await;
        });
    }

    loop {
        match listener.accept().await {
            Ok((mut stream, _)) => {
                if shared_state.max_requests_per_minute > 0 {
                    let mut limiter = shared_state.limiter.lock().await;
                    let addr = stream.peer_addr().unwrap().ip();
                    if !limiter.register_request(addr) {
                        let response = response::make_http_error(http::StatusCode::TOO_MANY_REQUESTS);
                        response::write_to_stream(&response, &mut stream).await.unwrap();
                        continue;
                    }
                }
                let shared_state_ref = shared_state.clone();
                tokio::spawn(async move {
                    handle_connection(stream, shared_state_ref).await
                });
            },
            Err(_) => { break; },
        }
    }
}

fn set_up_rate_limiter(limiter: ArgRateLimiter, max_requests_per_minute: usize) -> Box<dyn RateLimiterStrategy> {
    match limiter {
        ArgRateLimiter::Counter => {
            Box::new(Counter::new(max_requests_per_minute))
        }
    }
}

async fn limiter_refresh(state: Arc<ProxyState>, interval: u64) {
    sleep(Duration::from_secs(interval)).await;
    let mut limiter = state.limiter.lock().await;
    limiter.refresh()
}

async fn check_server(state: &Arc<ProxyState>, idx: usize, path: &String) -> Option<bool> {
    let addr = &state.upstream_addresses[idx];
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let request = http::Request::builder()
                .method(http::Method::GET)
                .uri(path)
                .header("Host", addr)
                .body(Vec::new())
                .unwrap();
        let _ = request::write_to_stream(&request, &mut stream).await.ok()?;
        let res = response::read_from_stream(&mut stream, &http::Method::GET).await.ok()?;
        if res .status().as_u16() != 200 {
            None
        } else {
            Some(true)
        }
    } else {
        None
    }
}

async fn active_health_check(state: Arc<ProxyState>) {
    let interval = state.active_health_check_interval as u64;
    let path = &state.active_health_check_path;
    loop {
        sleep(Duration::from_secs(interval)).await;
        let mut upstream_status = state.upstream_status.write().await;
        for idx in 0..upstream_status.status.len() {
            if check_server(&state, idx, path).await.is_some() {
                upstream_status.set_up(idx);
            } else {
                upstream_status.set_down(idx);
            }
        }
    }
}

async fn connect_to_upstream(state: Arc<ProxyState>) -> Result<TcpStream, std::io::Error> {
    loop {
        if let Some(idx) = state.load_balancer.select_backend(&state).await {
            let addr = &state.upstream_addresses[idx];
            match TcpStream::connect(addr).await {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    log::error!("Failed to connect to upstream {}: {}", addr, err);
                    let mut upstream_status = state.upstream_status.write().await;
                    upstream_status.set_down(idx);
                }
            }
        } else {
            return Err(std::io::Error::new(ErrorKind::Other, "All the upstream servers are down!"));
        }
    }
}

async fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("{} <- {}", client_ip, response::format_response_line(&response));
    if let Err(error) = response::write_to_stream(&response, client_conn).await {
        log::warn!("Failed to send response to client: {}", error);
        return;
    }
}

async fn handle_connection(mut client_conn: TcpStream, state: Arc<ProxyState>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // Open a connection to a random destination server
    let mut upstream_conn = match connect_to_upstream(state).await {
        Ok(stream) => stream,
        Err(_error) => {
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
    };
    let upstream_ip = client_conn.peer_addr().unwrap().ip().to_string();

    // The client may now send us one or more requests. Keep trying to read requests until the
    // client hangs up or we get an error.
    loop {
        // Read a request from the client
        let mut request = match request::read_from_stream(&mut client_conn).await {
            Ok(request) => request,
            // Handle case where client closed connection and is no longer sending requests
            Err(request::Error::IncompleteRequest(0)) => {
                log::debug!("Client finished sending requests. Shutting down connection");
                return;
            }
            // Handle I/O error in reading from the client
            Err(request::Error::ConnectionError(io_err)) => {
                log::info!("Error reading request from client stream: {}", io_err);
                return;
            }
            Err(error) => {
                log::debug!("Error parsing request: {:?}", error);
                let response = response::make_http_error(match error {
                    request::Error::IncompleteRequest(_)
                    | request::Error::MalformedRequest(_)
                    | request::Error::InvalidContentLength
                    | request::Error::ContentLengthMismatch => http::StatusCode::BAD_REQUEST,
                    request::Error::RequestBodyTooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
                    request::Error::ConnectionError(_) => http::StatusCode::SERVICE_UNAVAILABLE,
                });
                send_response(&mut client_conn, &response).await;
                continue;
            }
        };
        log::info!(
            "{} -> {}: {}",
            client_ip,
            upstream_ip,
            request::format_request_line(&request)
        );

        // Add X-Forwarded-For header so that the upstream server knows the client's IP address.
        // (We're the ones connecting directly to the upstream server, so without this header, the
        // upstream server will only know our IP, not the client's.)
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        // Forward the request to the server
        if let Err(error) = request::write_to_stream(&request, &mut upstream_conn).await {
            log::error!("Failed to send request to upstream {}: {}", upstream_ip, error);
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
        log::debug!("Forwarded request to server");

        // Read the server's response
        let response = match response::read_from_stream(&mut upstream_conn, request.method()).await {
            Ok(response) => response,
            Err(error) => {
                log::error!("Error reading response from server: {:?}", error);
                let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                send_response(&mut client_conn, &response).await;
                return;
            }
        };
        // Forward the response to the client
        send_response(&mut client_conn, &response).await;
        log::debug!("Forwarded response to client");
    }
}
