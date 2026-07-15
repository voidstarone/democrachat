//! A media file attached to a message.

use serde::{Deserialize, Serialize};

use crate::MediaKind;

/// One media attachment on a [`crate::Message`]. The bytes live in the media store
/// under `key` (served at `/media/{key}`); the message only carries this reference,
/// so deleting the message can delete the blob (media lives and dies with its
/// message). `is_spoiler` asks the client to blur it until the viewer reveals it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Attachment {
    /// Opaque storage key in the media store.
    pub key: String,
    /// The stored MIME type, used for the `Content-Type` when serving and to pick
    /// the render element.
    pub content_type: String,
    pub kind: MediaKind,
    /// Optional alt text / caption.
    #[serde(default)]
    pub caption: String,
    /// Render blurred, click-to-reveal.
    #[serde(default)]
    pub is_spoiler: bool,
}

impl Attachment {
    pub fn new(
        key: impl Into<String>,
        content_type: impl Into<String>,
        kind: MediaKind,
        caption: impl Into<String>,
        is_spoiler: bool,
    ) -> Self {
        Self {
            key: key.into(),
            content_type: content_type.into(),
            kind,
            caption: caption.into(),
            is_spoiler,
        }
    }
}
