//! Persistence for channels.

use domain::{Channel, ChannelId, ServerId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for channels.
#[async_trait]
pub trait ChannelStore: Send + Sync {
    async fn next_channel_id(&self) -> Result<ChannelId, StoreError>;
    async fn insert_channel(&self, channel: Channel) -> Result<(), StoreError>;
    async fn get_channel(&self, id: ChannelId) -> Result<Option<Channel>, StoreError>;
    async fn find_by_name(&self, server: ServerId, name: &str) -> Result<Option<Channel>, StoreError>;
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Channel>, StoreError>;
    /// Delete a channel by id (a passed DeleteChannel ballot). Returns whether it
    /// existed. Messages in it are left orphaned — the store may prune them.
    async fn remove_channel(&self, id: ChannelId) -> Result<bool, StoreError>;
    /// Channels carrying the exact tag `tag` (a plain tag — the store handles any
    /// normalization/fencing internally). Global across servers, for discovery;
    /// order is unspecified.
    async fn search_by_tag(&self, tag: &str) -> Result<Vec<Channel>, StoreError>;
}
