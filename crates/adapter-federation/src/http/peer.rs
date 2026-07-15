//! One configured peer to replicate from.

use domain::NodeId;

use crate::http::feed_client::FeedClient;

/// A peer this node pulls from: its node id (the replication-cursor key) and a
/// client for its feed.
pub struct Peer {
    pub node: NodeId,
    pub client: FeedClient,
}
