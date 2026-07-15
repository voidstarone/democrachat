//! Assemble a flat list of messages into threaded trees.

use std::collections::HashMap;

use crate::{Message, MessageId, MessageNode};

/// Build a forest of [`MessageNode`]s from a channel's flat message list.
///
/// Top-level messages (`parent = None`) become roots; every other message nests
/// under its parent. Within each level, messages are ordered by their id (which
/// the store assigns monotonically, so id order is post order). A reply whose
/// parent is missing from `messages` (e.g. a hard-deleted ancestor that wasn't
/// tombstoned) is promoted to a root so it is never lost. Pure and deterministic.
pub fn build_message_tree(messages: &[Message]) -> Vec<MessageNode> {
    // Children grouped by parent id; `None` bucket holds the roots.
    let mut children: HashMap<Option<MessageId>, Vec<&Message>> = HashMap::new();
    let present: std::collections::HashSet<MessageId> = messages.iter().map(|m| m.id).collect();

    for m in messages {
        // Treat a reply to a missing parent as a root.
        let key = match m.parent {
            Some(p) if present.contains(&p) => Some(p),
            _ => None,
        };
        children.entry(key).or_default().push(m);
    }
    for bucket in children.values_mut() {
        bucket.sort_by_key(|m| m.id.0);
    }

    build_level(None, &children)
}

fn build_level(
    parent: Option<MessageId>,
    children: &HashMap<Option<MessageId>, Vec<&Message>>,
) -> Vec<MessageNode> {
    children
        .get(&parent)
        .into_iter()
        .flatten()
        .map(|m| MessageNode {
            message: (*m).clone(),
            replies: build_level(Some(m.id), children),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelId, ServerId, UserId};

    fn msg(id: u64, parent: Option<u64>) -> Message {
        Message::new(
            MessageId(id),
            ChannelId(1),
            ServerId(1),
            UserId(1),
            format!("m{id}"),
            parent.map(MessageId),
            crate::Timestamp(id as i64),
        )
    }

    #[test]
    fn nests_replies_under_parents_in_id_order() {
        // 1 (root) -> 2, 3 ; 2 -> 4
        let msgs = vec![msg(3, Some(1)), msg(1, None), msg(4, Some(2)), msg(2, Some(1))];
        let tree = build_message_tree(&msgs);
        assert_eq!(tree.len(), 1);
        let root = &tree[0];
        assert_eq!(root.message.id, MessageId(1));
        assert_eq!(root.replies.len(), 2);
        assert_eq!(root.replies[0].message.id, MessageId(2));
        assert_eq!(root.replies[1].message.id, MessageId(3));
        assert_eq!(root.replies[0].replies[0].message.id, MessageId(4));
    }

    #[test]
    fn a_reply_to_a_missing_parent_is_promoted_to_a_root() {
        let msgs = vec![msg(5, Some(99))]; // parent 99 absent
        let tree = build_message_tree(&msgs);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].message.id, MessageId(5));
    }
}
