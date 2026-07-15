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
    /// The bytes were declared an image but could not be decoded as one (corrupt,
    /// hostile, or larger than the decode-bomb guard allows).
    #[error("image could not be processed")]
    Undecodable,
    /// The media store failed to read or write the blob.
    #[error("media storage error")]
    Io,
}
