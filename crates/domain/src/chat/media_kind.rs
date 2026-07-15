//! The broad kind of a media attachment, derived from its MIME type.

use serde::{Deserialize, Serialize};

/// What sort of media an [`crate::Attachment`] is, so a client knows whether to
/// render an `<img>`, `<video>`, or `<audio>` element. Classified from the MIME
/// top-level type at upload time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    /// Classify a MIME `content_type` by its top-level type. `None` for anything
    /// that is not image/video/audio — the caller rejects those uploads.
    pub fn from_content_type(content_type: &str) -> Option<Self> {
        match content_type.split('/').next()?.trim().to_ascii_lowercase().as_str() {
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_mime_top_level_type() {
        assert_eq!(MediaKind::from_content_type("image/png"), Some(MediaKind::Image));
        assert_eq!(MediaKind::from_content_type("VIDEO/mp4"), Some(MediaKind::Video));
        assert_eq!(MediaKind::from_content_type("audio/ogg"), Some(MediaKind::Audio));
        assert_eq!(MediaKind::from_content_type("application/pdf"), None);
        assert_eq!(MediaKind::from_content_type("text/html"), None);
    }
}
