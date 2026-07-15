//! Double-submit CSRF protection for the JSON API.
//!
//! On every state-changing request the client must send an `X-DC-CSRF-Token` header
//! whose value equals its non-HttpOnly `dc_csrf` cookie. A cross-site attacker can
//! neither read the victim's cookie (same-origin policy) to echo it in the
//! header, nor — thanks to `SameSite=Lax` — cause the cookie to ride along on a
//! cross-site POST. The token is client-generated, so the server holds no state
//! and only compares the two in constant time. Safe methods (GET/HEAD) pass
//! through untouched.

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::{cookie_value, CSRF_COOKIE, CSRF_HEADER};

pub async fn csrf(req: Request, next: Next) -> Response {
    let is_mutation = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if is_mutation {
        let cookie = cookie_value(req.headers(), CSRF_COOKIE);
        let header = req
            .headers()
            .get(CSRF_HEADER)
            .and_then(|v| v.to_str().ok());
        let ok = match (cookie, header) {
            (Some(c), Some(h)) if !c.is_empty() => app::constant_time_eq(c.as_bytes(), h.as_bytes()),
            _ => false,
        };
        if !ok {
            return (StatusCode::FORBIDDEN, "CSRF check failed").into_response();
        }
    }
    next.run(req).await
}
