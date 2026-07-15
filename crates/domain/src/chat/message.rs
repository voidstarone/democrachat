//! A message in a channel — the primary content type, replacing posts/comments.

use serde::{Deserialize, Serialize};

use crate::{Attachment, ChannelId, ServerId, MessageId, Timestamp, UserId};

/// A single message. A top-level message has `parent = None`; a threaded reply
/// carries the id of the message it replies to, so a channel's messages form a
/// tree (see [`crate::build_message_tree`]) exactly as comments did.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub server_id: ServerId,
    pub author: UserId,
    /// The message body. Plaintext when `key_epoch` is `None`; otherwise the
    /// ciphertext (nonce ‖ AEAD output) sealed under the channel key of that epoch —
    /// opaque to the server, which never holds the key.
    pub body: String,
    /// The channel-key epoch the body is encrypted under. `None` means the body is
    /// plaintext (an un-encrypted channel); `Some(e)` means it is sealed under
    /// epoch `e`'s channel key, so a reader knows which grant to open.
    #[serde(default)]
    pub key_epoch: Option<u32>,
    /// The message this one replies to (its thread parent), if any.
    #[serde(default)]
    pub parent: Option<MessageId>,
    pub created_at: Timestamp,
    /// When the body was last edited, if ever.
    #[serde(default)]
    pub edited_at: Option<Timestamp>,
    /// A deleted message is tombstoned, not removed, so replies keep their place
    /// in the thread. The body is blanked on delete.
    #[serde(default)]
    pub is_deleted: bool,
    /// Ordered media attachments. Their bytes live in the media store keyed by
    /// [`Attachment::key`]; deleting the message deletes them (see
    /// [`tombstone`](Self::tombstone)). Empty for a text-only message. Only
    /// permitted on plaintext (non-sealed) channels for now.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl Message {
    pub fn new(
        id: MessageId,
        channel_id: ChannelId,
        server_id: ServerId,
        author: UserId,
        body: impl Into<String>,
        parent: Option<MessageId>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            channel_id,
            server_id,
            author,
            body: body.into(),
            key_epoch: None,
            parent,
            created_at,
            edited_at: None,
            is_deleted: false,
            attachments: Vec::new(),
        }
    }

    /// Build a message whose `body` is already ciphertext, sealed under the given
    /// channel-key `epoch`. The server never sees the plaintext or the key.
    #[allow(clippy::too_many_arguments)]
    pub fn sealed(
        id: MessageId,
        channel_id: ChannelId,
        server_id: ServerId,
        author: UserId,
        ciphertext: impl Into<String>,
        key_epoch: u32,
        parent: Option<MessageId>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            channel_id,
            server_id,
            author,
            body: ciphertext.into(),
            key_epoch: Some(key_epoch),
            parent,
            created_at,
            edited_at: None,
            is_deleted: false,
            attachments: Vec::new(),
        }
    }

    /// Replace the body and stamp the edit time.
    pub fn edit(&mut self, body: impl Into<String>, at: Timestamp) {
        self.body = body.into();
        self.edited_at = Some(at);
    }

    /// Tombstone the message: blank the body and mark it deleted. Kept in place so
    /// its replies remain threaded.
    pub fn tombstone(&mut self) {
        self.body.clear();
        self.attachments.clear();
        self.is_deleted = true;
    }
}
