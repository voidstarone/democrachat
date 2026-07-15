//! A node's change-capture outbox — the source of its replication feed.

use async_trait::async_trait;

use crate::change_record::ChangeRecord;

/// A node's outbox: the ordered log of every mutation it has made, which
/// [`sign_feed`](crate::sign_feed::sign_feed) turns into a signed feed for peers
/// to pull. The store implements this; the feed logic stays store-agnostic.
#[async_trait]
pub trait ChangeSource: Send + Sync {
    /// Outbox records with `seq > after_seq`, in ascending `seq` order, at most
    /// `limit`. A peer pulls repeatedly, advancing `after_seq` to the highest seq
    /// it has applied (its per-origin cursor).
    async fn changes_since(&self, after_seq: u64, limit: u64) -> Vec<ChangeRecord>;
}
