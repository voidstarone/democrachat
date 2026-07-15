//! A node in a rendered message thread.

use crate::Message;

/// A message together with its replies, nested. Produced by
/// [`crate::build_message_tree`] for display; not persisted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MessageNode {
    pub message: Message,
    pub replies: Vec<MessageNode>,
}
