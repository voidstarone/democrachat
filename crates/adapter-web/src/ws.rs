//! The realtime gateway: a WebSocket that both **broadcasts** server events to the
//! browser (the client re-fetches the affected view when it sees one) and carries
//! **targeted** voice signaling — SDP offers/answers and ICE candidates relayed
//! between the members of a voice channel's roster (see [`crate::signal`]).

use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::state::AppState;

pub async fn ws_handler(
    State(st): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Cross-site WebSocket hijacking defense: the WS upgrade is exempt from CORS
    // and SameSite, so we check the Origin's host against the request Host and
    // reject a mismatch. A same-origin browser sends a matching Origin.
    if !origin_ok(&headers) {
        return (StatusCode::FORBIDDEN, "err.bad_origin").into_response();
    }
    // Require a valid session — the live feed is for authenticated users only.
    let Some(handle) = crate::auth::current_actor(&st, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "err.not_signed_in").into_response();
    };
    upgrade.on_upgrade(move |socket| pump(socket, st, handle))
}

/// Whether the `Origin` header's host matches the request `Host`. A missing
/// Origin (non-browser client) is allowed; a present, mismatched one is rejected.
fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true; // no Origin → not a browser cross-site request
    };
    // Strip scheme, keep authority (host[:port]).
    let origin_host = origin.split("://").nth(1).unwrap_or(origin);
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    !host.is_empty() && origin_host == host
}

/// Signaling rate limit: a token bucket, one token per inbound voice frame. Bursts up
/// to [`BUCKET_CAP`] are fine (mesh setup for a full room is chatty); the sustained
/// rate is [`REFILL_PER_SEC`]. A client that outruns it has its excess frames dropped
/// — signaling is best-effort and the client retries — rather than being disconnected.
const BUCKET_CAP: f64 = 60.0;
const REFILL_PER_SEC: f64 = 30.0;

struct RateLimiter {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    fn new() -> Self {
        Self { tokens: BUCKET_CAP, last: Instant::now() }
    }

    /// Try to spend one token, refilling by elapsed wall time first. `true` if the
    /// frame is allowed, `false` if the bucket is empty (drop the frame).
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * REFILL_PER_SEC)
            .min(BUCKET_CAP);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

async fn pump(mut socket: WebSocket, st: AppState, handle: String) {
    let mut rx = st.events.subscribe();
    // Register this connection with the signaling hub and take our private outbound
    // queue (targeted signaling lands here). Unregister on any exit path.
    let (conn_id, mut mail) = st.signal.register(handle);
    let mut limiter = RateLimiter::new();

    // Announce presence to *everyone* so open clients refresh their active-users
    // roster when someone joins (not just this socket's own count).
    st.publish(presence_event(&st));

    loop {
        tokio::select! {
            // Broadcast server event -> forward to this client.
            evt = rx.recv() => match evt {
                Ok(text) => {
                    if socket.send(Message::Text(text)).await.is_err() {
                        break; // client went away
                    }
                }
                Err(_) => break, // lagged or closed
            },
            // Targeted signaling addressed to this connection -> forward.
            msg = mail.recv() => match msg {
                Some(text) => {
                    if socket.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                None => break, // hub dropped our sender (unregistered)
            },
            // Client -> handle voice signaling frames; watch for close.
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Text(text))) => {
                    if limiter.allow() {
                        handle_inbound(&st, conn_id, &text);
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {} // ping/pong/binary ignored
            },
        }
    }

    st.signal.unregister(conn_id);
    // Tell everyone else this connection left, so their rosters drop the user.
    st.publish(presence_event(&st));
}

/// The presence frame: a live connection count plus the distinct online handles,
/// so each client can intersect the roster with the server it is viewing.
fn presence_event(st: &AppState) -> String {
    let online = st.events.receiver_count();
    let handles = st.signal.online_handles();
    serde_json::json!({ "type": "presence", "online": online, "handles": handles }).to_string()
}

/// Parse and act on one client frame. Only voice signaling is accepted inbound; an
/// unknown or malformed frame is ignored. `server`/`channel` name the voice room;
/// the hub validates that this connection is actually a member before relaying.
fn handle_inbound(st: &AppState, conn_id: u64, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let server = v.get("server").and_then(Value::as_str).unwrap_or("");
    let channel = v.get("channel").and_then(Value::as_str).unwrap_or("");
    if server.is_empty() || channel.is_empty() {
        return;
    }
    match kind {
        "voice_join" => st.signal.join(conn_id, server, channel),
        "voice_leave" => st.signal.leave(conn_id, server, channel),
        "voice_signal" => {
            let Some(to) = v.get("to").and_then(Value::as_u64) else { return };
            let data = v.get("data").cloned().unwrap_or(Value::Null);
            st.signal.relay(conn_id, server, channel, to, data);
        }
        _ => {}
    }
}
