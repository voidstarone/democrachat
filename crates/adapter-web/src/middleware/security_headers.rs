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

// `blob:` on img-src/media-src lets the composer preview a picked image or video
// locally (via `URL.createObjectURL`) before it is uploaded; the blob is a
// same-origin, in-memory handle, not a network fetch.
const CSP: &str = "default-src 'self'; \
img-src 'self' https: data: blob:; \
media-src 'self' https: blob:; \
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
    // Don't clobber a stricter policy a handler set for itself (e.g. the media
    // route sandboxes served blobs); only supply the app-wide default otherwise.
    if !h.contains_key(header::CONTENT_SECURITY_POLICY) {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    }
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

#[cfg(test)]
mod tests {
    //! Regression guards on the policy strings. These pin the security-relevant
    //! directives so a well-meaning relaxation (an inline-script allowance, a
    //! dropped frame guard) has to break a test on the way in.

    use super::{CSP, PERMISSIONS_POLICY};

    /// The CSP keeps the anti-XSS backstops: same-origin default, no plugins, no
    /// framing, locked base URI and form target.
    #[test]
    fn the_csp_pins_the_injection_backstops() {
        for directive in [
            "default-src 'self'",
            "object-src 'none'",
            "base-uri 'self'",
            "frame-ancestors 'none'",
            "form-action 'self'",
            "connect-src 'self'",
        ] {
            assert!(CSP.contains(directive), "CSP must keep `{directive}`");
        }
    }

    /// No inline or eval'd script is ever allowed: `script-src` carries only 'self'
    /// and the narrow wasm relaxation — never 'unsafe-inline' or 'unsafe-eval'.
    #[test]
    fn script_src_forbids_inline_and_eval() {
        assert!(CSP.contains("script-src 'self' 'wasm-unsafe-eval'"));
        // The one 'unsafe-inline' the policy tolerates is on style-src, not script-src.
        let script_src = CSP
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("script-src"))
            .expect("a script-src directive is present");
        assert!(!script_src.contains("'unsafe-inline'"), "no inline script may run");
        assert!(!script_src.contains("'unsafe-eval'"), "no eval/new Function may run");
    }

    /// The Permissions-Policy denies the powerful device features outright.
    #[test]
    fn the_permissions_policy_denies_device_features() {
        for feature in ["camera=()", "microphone=()", "geolocation=()", "payment=()"] {
            assert!(PERMISSIONS_POLICY.contains(feature), "must deny `{feature}`");
        }
    }
}
