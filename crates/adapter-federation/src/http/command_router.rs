//! The command endpoint: run a forwarded write on the owner.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};

use crate::command::execute::execute;
use crate::command::forward_error::ForwardError;
use crate::command::signed_command::SignedCommand;
use crate::http::bearer_ok::bearer_ok;
use crate::http::command_state::CommandState;

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Map a forwarding failure to an HTTP status the client can act on: a `Rejected`
/// command is the client's fault (bad signature / not-owner / replay), `Unowned`
/// is a routing race worth retrying, and `OwnerUnreachable` is an owner-side blip.
fn status_for(err: &ForwardError) -> StatusCode {
    match err {
        ForwardError::Rejected(_) => StatusCode::UNPROCESSABLE_ENTITY,
        ForwardError::Unowned => StatusCode::CONFLICT,
        ForwardError::OwnerUnreachable(_) => StatusCode::BAD_GATEWAY,
    }
}

async fn command_handler(
    State(state): State<CommandState>,
    headers: HeaderMap,
    Json(signed): Json<SignedCommand>,
) -> StatusCode {
    if !bearer_ok(state.token.as_deref(), &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    match execute(
        state.node,
        state.registry.as_ref(),
        state.resolver.as_ref(),
        &state.replay,
        state.executor.as_ref(),
        &signed,
        unix_now(),
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(e) => {
            tracing::warn!("forwarded command refused: {e}");
            status_for(&e)
        }
    }
}

/// The federation command router. Mount on a **node-only** address.
pub fn command_router(state: CommandState) -> Router {
    Router::new()
        .route("/federation/command", post(command_handler))
        .with_state(state)
}
