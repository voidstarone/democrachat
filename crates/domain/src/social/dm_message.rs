//! A direct message between two users.

use serde::{Deserialize, Serialize};

use crate::{DmId, Timestamp, UserId};

/// One direct message, from `sender` to `recipient`. DMs live outside any server
/// — they are a platform-wide conversation between two accounts — so this entity
/// names no [`ServerId`](crate::ServerId).
///
/// The body is **end-to-end encrypted**: it is carried as two ciphertexts, each a
/// sealed box the client produced against a device public key from the key
/// directory. The server (and every replicating node) only ever holds these
/// opaque blobs — it has no key to open them. The metadata (`sender`, `recipient`,
/// `created_at`) stays in the clear so the platform can still route, gate, and
/// order the conversation.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DmMessage {
    pub id: DmId,
    pub sender: UserId,
    pub recipient: UserId,
    /// The body sealed to the **recipient's** device key — only they can open it.
    pub sealed_for_recipient: String,
    /// The body sealed to the **sender's** device key, so the sender can re-read
    /// their own sent message (a sealed box is not openable by its sender).
    pub sealed_for_sender: String,
    pub created_at: Timestamp,
}

impl DmMessage {
    pub fn new(
        id: DmId,
        sender: UserId,
        recipient: UserId,
        sealed_for_recipient: impl Into<String>,
        sealed_for_sender: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            sender,
            recipient,
            sealed_for_recipient: sealed_for_recipient.into(),
            sealed_for_sender: sealed_for_sender.into(),
            created_at,
        }
    }

    /// Whether this message is part of the conversation between `a` and `b`,
    /// regardless of who sent it.
    pub fn is_between(&self, a: UserId, b: UserId) -> bool {
        (self.sender == a && self.recipient == b) || (self.sender == b && self.recipient == a)
    }
}
