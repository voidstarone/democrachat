//! Bind and serve the federation feed until the process exits.

use crate::http::feed_router::feed_router;
use crate::http::feed_state::FeedState;

/// Serve the feed on `addr` (e.g. a node-only `10.x` bind) forever.
pub async fn serve_feed(state: FeedState, addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, feed_router(state)).await
}
