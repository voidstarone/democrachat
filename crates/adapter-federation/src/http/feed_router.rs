//! The feed endpoint: serve this node's signed change feed to an authenticated peer.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use federation::{sign_feed, ChangeEvent};

use crate::http::bearer_ok::bearer_ok;
use crate::http::feed_state::FeedState;

#[derive(Deserialize)]
struct ChangesQuery {
    since: Option<u64>,
    limit: Option<u64>,
}

async fn changes_handler(
    State(state): State<FeedState>,
    headers: HeaderMap,
    Query(q): Query<ChangesQuery>,
) -> Result<Json<Vec<ChangeEvent>>, StatusCode> {
    if !bearer_ok(state.token.as_deref(), &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let since = q.since.unwrap_or(0);
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000);
    let events = sign_feed(
        state.store.as_ref(),
        &state.keypair,
        state.registry.as_ref(),
        state.resolver.as_ref(),
        since,
        limit,
    )
    .await;
    Ok(Json(events))
}

/// The federation feed router. Mount it on a **node-only** address — the feed is
/// signed but not encrypted here, and the bearer token is the only gate.
pub fn feed_router(state: FeedState) -> Router {
    Router::new()
        .route("/federation/changes", get(changes_handler))
        .with_state(state)
}
