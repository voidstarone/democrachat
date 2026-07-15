//! A dependency-free, per-IP fixed-window rate limiter for POST requests.
//!
//! Two buckets: `Auth` (login/register) is throttled hard because those paths run
//! Argon2 and are the online-guessing surface; `Write` covers every other
//! mutation. The limiter is keyed on the **direct connection peer** IP, never on
//! `X-Forwarded-For` (which a client can spoof) — a proxied deployment must do
//! edge limiting or resolve the real peer at the proxy. Over-limit yields `429`
//! with `Retry-After`, rejecting the request before any expensive work runs.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, State};
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Auth bucket: 10 requests per 60s. Write bucket: 120 per 60s.
const AUTH_MAX: u32 = 10;
const WRITE_MAX: u32 = 120;
const WINDOW: Duration = Duration::from_secs(60);
/// Prune the table once it grows past this many keys, to bound memory under IP
/// churn.
const PRUNE_THRESHOLD: usize = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Auth,
    Write,
}

impl Bucket {
    fn limit(self) -> u32 {
        match self {
            Bucket::Auth => AUTH_MAX,
            Bucket::Write => WRITE_MAX,
        }
    }
}

struct Window {
    count: u32,
    reset_at: Instant,
}

/// Per-IP, per-bucket fixed-window counters.
pub struct RateLimiter {
    windows: Mutex<HashMap<(IpAddr, Bucket), Window>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { windows: Mutex::new(HashMap::new()) }
    }

    /// Record a hit; on success `Ok(())`, on over-limit `Err(retry_after_secs)`.
    fn check(&self, ip: IpAddr, bucket: Bucket, now: Instant) -> Result<(), u64> {
        let mut windows = self.windows.lock().unwrap();
        if windows.len() > PRUNE_THRESHOLD {
            windows.retain(|_, w| w.reset_at > now);
        }
        let w = windows
            .entry((ip, bucket))
            .or_insert_with(|| Window { count: 0, reset_at: now + WINDOW });
        if w.reset_at <= now {
            w.count = 0;
            w.reset_at = now + WINDOW;
        }
        if w.count >= bucket.limit() {
            return Err(w.reset_at.saturating_duration_since(now).as_secs().max(1));
        }
        w.count += 1;
        Ok(())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Which bucket a POST path falls into.
fn bucket_for(path: &str) -> Bucket {
    if path == "/api/login" || path == "/api/register" {
        Bucket::Auth
    } else {
        Bucket::Write
    }
}

/// Middleware: throttle POSTs by peer IP. Non-POST requests pass through.
pub async fn rate_limit(
    State(limiter): State<Arc<RateLimiter>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if req.method() == Method::POST {
        let bucket = bucket_for(req.uri().path());
        if let Err(retry) = limiter.check(peer.ip(), bucket, Instant::now()) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry.to_string())],
                "rate limit exceeded — slow down",
            )
                .into_response();
        }
    }
    next.run(req).await
}
