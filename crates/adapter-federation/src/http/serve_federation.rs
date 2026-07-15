//! Serve both federation endpoints — feed pull and command forward — on one
//! node-only address.

use crate::http::command_router::command_router;
use crate::http::command_state::CommandState;
use crate::http::feed_router::feed_router;
use crate::http::feed_state::FeedState;

/// Bind `addr` and serve the feed (`GET /federation/changes`) and command
/// (`POST /federation/command`) routes together until the process exits. Both are
/// node-only surfaces gated by the shared bearer token.
pub async fn serve_federation(
    feed: FeedState,
    command: CommandState,
    addr: &str,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = feed_router(feed).merge(command_router(command));
    axum::serve(listener, app).await
}
