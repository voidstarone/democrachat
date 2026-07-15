//! Persistence for channels.

use domain::{Channel, ChannelId, ServerId};

/// Persistence for channels.
pub trait ChannelStore: Send + Sync {
    fn next_channel_id(&self) -> ChannelId;
    fn insert_channel(&self, channel: Channel);
    fn get_channel(&self, id: ChannelId) -> Option<Channel>;
    fn find_by_name(&self, server: ServerId, name: &str) -> Option<Channel>;
    fn list_for_server(&self, server: ServerId) -> Vec<Channel>;
    /// Delete a channel by id (a passed DeleteChannel ballot). Returns whether it
    /// existed. Messages in it are left orphaned — the store may prune them.
    fn remove_channel(&self, id: ChannelId) -> bool;
}
