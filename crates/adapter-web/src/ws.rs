//! The realtime gateway: a WebSocket that forwards broadcast events to the
//! browser. The client re-fetches the affected view when it sees an event, which
//! keeps the wire format trivial and the state authoritative on the server.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

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
    if crate::auth::current_actor(&st, &headers).await.is_none() {
        return (StatusCode::UNAUTHORIZED, "err.not_signed_in").into_response();
    }
    upgrade.on_upgrade(move |socket| pump(socket, st))
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

async fn pump(mut socket: WebSocket, st: AppState) {
    let mut rx = st.events.subscribe();
    // Announce presence so clients can show a live connection count.
    let online = st.events.receiver_count();
    let _ = socket
        .send(Message::Text(format!("{{\"type\":\"presence\",\"online\":{online}}}")))
        .await;

    loop {
        tokio::select! {
            // Server-side event -> forward to this client.
            evt = rx.recv() => match evt {
                Ok(text) => {
                    if socket.send(Message::Text(text)).await.is_err() {
                        break; // client went away
                    }
                }
                Err(_) => break, // lagged or closed
            },
            // Client -> we only watch for a close; inbound frames are ignored.
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            },
        }
    }
}
