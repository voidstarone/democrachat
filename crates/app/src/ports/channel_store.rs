//! Persistence for channels.

use domain::{Channel, ChannelId, ServerId};
use crate::StoreError;

/// Persistence for channels.
pub trait ChannelStore: Send + Sync {
    fn next_channel_id(&self) -> Result<ChannelId, StoreError>;
    fn insert_channel(&self, channel: Channel) -> Result<(), StoreError>;
    fn get_channel(&self, id: ChannelId) -> Result<Option<Channel>, StoreError>;
    fn find_by_name(&self, server: ServerId, name: &str) -> Result<Option<Channel>, StoreError>;
    fn list_for_server(&self, server: ServerId) -> Result<Vec<Channel>, StoreError>;
    /// Delete a channel by id (a passed DeleteChannel ballot). Returns whether it
    /// existed. Messages in it are left orphaned — the store may prune them.
    fn remove_channel(&self, id: ChannelId) -> Result<bool, StoreError>;
}
