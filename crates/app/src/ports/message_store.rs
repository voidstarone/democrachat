//! Persistence for messages.

use domain::{ChannelId, Message, MessageId};

/// Persistence for messages.
pub trait MessageStore: Send + Sync {
    fn next_message_id(&self) -> MessageId;
    fn insert_message(&self, message: Message);
    fn get_message(&self, id: MessageId) -> Option<Message>;
    /// Replace an existing message (edit/tombstone).
    fn update_message(&self, message: Message);
    /// All messages in a channel, in id order (which is post order).
    fn list_for_channel(&self, channel: ChannelId) -> Vec<Message>;
}
