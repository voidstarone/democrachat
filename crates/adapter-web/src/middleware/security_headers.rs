//! One middleware that stamps the security headers onto every response.
//!
//! The Content-Security-Policy is the anti-XSS backstop behind the client's own
//! output escaping. `object-src 'none'`, `base-uri 'self'`, `frame-ancestors
//! 'none'` and `form-action 'self'` shut down the classic injection escalations;
//! `default-src 'self'` keeps the app same-origin. `script-src 'self'` allows no
//! inline script at all: the client's JS is served from `/app.js` and every
//! handler is attached via event delegation, so an injected inline `<script>` or
//! `onclick=` is inert, and any cross-origin `<script src=…>` is blocked. The
//! `'wasm-unsafe-eval'` on `script-src` is the minimal relaxation that lets the
//! browser compile our own same-origin WebAssembly (the E2EE crypto); it permits
//! *only* wasm compilation, not JS `eval`/`new Function`, so the XSS surface is
//! unchanged. The lone `'unsafe-inline'` left is on `style-src`, for the SPA's
//! inline `style=` attributes (colours, widths) — inert markup that can't run script.

use axum::extract::Request;
use axum::http::{header, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

const CSP: &str = "default-src 'self'; \
img-src 'self' https: data:; \
media-src 'self' https:; \
script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; \
connect-src 'self'; \
object-src 'none'; \
base-uri 'self'; \
frame-ancestors 'none'; \
form-action 'self'";

const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), payment=()";

pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    );
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
    res
}
