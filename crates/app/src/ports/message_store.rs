//! Persistence for messages.

use domain::{ChannelId, Message, MessageId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for messages.
#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn next_message_id(&self) -> Result<MessageId, StoreError>;
    async fn insert_message(&self, message: Message) -> Result<(), StoreError>;
    async fn get_message(&self, id: MessageId) -> Result<Option<Message>, StoreError>;
    /// Replace an existing message (edit/tombstone).
    async fn update_message(&self, message: Message) -> Result<(), StoreError>;
    /// All messages in a channel, in id order (which is post order).
    async fn list_for_channel(&self, channel: ChannelId) -> Result<Vec<Message>, StoreError>;
}
