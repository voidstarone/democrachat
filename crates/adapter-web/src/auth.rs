//! Turning the session cookie into the request's authenticated identity, and
//! building the `Set-Cookie` headers for login/logout.
//!
//! Every mutating handler derives the actor from [`require_actor`] — never from a
//! field in the request body. This is what closes the impersonation and DM-IDOR
//! holes: a client can *say* it is `@alice`, but only a validly-signed `sid`
//! cookie makes it so.

use axum::http::{header, HeaderMap, StatusCode};
use domain::UserId;

use crate::state::AppState;

/// The session cookie name.
pub const SESSION_COOKIE: &str = "sid";
/// The CSRF cookie name (readable by JS; echoed back in the `X-DC-CSRF-Token`
/// header and compared by the CSRF middleware). **App-namespaced on purpose**: a
/// sibling app on the same `localhost:PORT` origin (e.g. democratos) sets a plain
/// `csrf` cookie — often HttpOnly — which our JS could neither read nor overwrite,
/// so its stale value would ride along and fail the double-submit. A unique name
/// keeps our cookie ours.
pub const CSRF_COOKIE: &str = "dc_csrf";
/// The CSRF header name the client echoes the cookie in (namespaced to match).
pub const CSRF_HEADER: &str = "x-dc-csrf-token";
/// Session lifetime: 30 days, matching the democratos sibling.
pub const SESSION_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Extract a cookie value from a `Cookie` header.
pub fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k.trim() == name {
            Some(v.trim())
        } else {
            None
        }
    })
}

/// The authenticated actor's handle, or `None` if the request carries no valid,
/// unexpired session. Reads the `sid` cookie, verifies its HMAC, checks expiry
/// against the server clock, and resolves the uid to a current handle.
pub fn current_actor(st: &AppState, headers: &HeaderMap) -> Option<String> {
    let token = cookie_value(headers, SESSION_COOKIE)?;
    let (uid, expires_at) = st.signer.verify(token)?;
    if expires_at < st.services.now().0 {
        return None; // expired — reject even if the browser resent it
    }
    st.services.chat().user_handle(UserId(uid))
}

/// Require an authenticated actor, or fail with `401`.
pub fn require_actor(st: &AppState, headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    current_actor(st, headers).ok_or((StatusCode::UNAUTHORIZED, "err.not_signed_in".to_string()))
}

/// Build the `Set-Cookie` value for a freshly signed session.
pub fn session_cookie(st: &AppState, uid: u64) -> String {
    let expires_at = st.services.now().0 + SESSION_TTL_SECONDS;
    let token = st.signer.sign(uid, expires_at);
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECONDS}{}",
        secure_suffix(st)
    )
}

/// Build the `Set-Cookie` value that clears the session (logout).
pub fn clear_session_cookie(st: &AppState) -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}", secure_suffix(st))
}

fn secure_suffix(st: &AppState) -> &'static str {
    if st.secure_cookies {
        "; Secure"
    } else {
        ""
    }
}
