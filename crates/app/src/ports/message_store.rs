//! Persistence for messages.

use domain::{ChannelId, Message, MessageId};
use crate::StoreError;

/// Persistence for messages.
pub trait MessageStore: Send + Sync {
    fn next_message_id(&self) -> Result<MessageId, StoreError>;
    fn insert_message(&self, message: Message) -> Result<(), StoreError>;
    fn get_message(&self, id: MessageId) -> Result<Option<Message>, StoreError>;
    /// Replace an existing message (edit/tombstone).
    fn update_message(&self, message: Message) -> Result<(), StoreError>;
    /// All messages in a channel, in id order (which is post order).
    fn list_for_channel(&self, channel: ChannelId) -> Result<Vec<Message>, StoreError>;
}
