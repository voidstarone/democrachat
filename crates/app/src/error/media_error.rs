//! Errors from storing an uploaded media attachment.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MediaError {
    /// The upload has no bytes.
    #[error("empty upload")]
    Empty,
    /// The upload exceeds the per-file size cap.
    #[error("attachment too large")]
    TooLarge,
    /// The MIME type is not a supported image/video/audio type.
    #[error("unsupported media type: '{0}'")]
    UnsupportedType(String),
    /// The media store failed to read or write the blob.
    #[error("media storage error")]
    Io,
}
