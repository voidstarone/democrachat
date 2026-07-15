//! Constant-work bearer-token check for the node-only federation endpoints.

use axum::http::HeaderMap;

/// Whether the request carries the expected cluster bearer token. `expected ==
/// None` disables the check (dev/local only). Compares the raw `Authorization`
/// header against `Bearer <token>`.
pub fn bearer_ok(expected: Option<&str>, headers: &HeaderMap) -> bool {
    let Some(expected) = expected else {
        return true; // no token configured — checks are off
    };
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    presented == Some(expected)
}
