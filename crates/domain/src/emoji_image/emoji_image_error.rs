//! Why an uploaded emoji image was rejected.

/// A rejection reason for an uploaded custom-emoji image. `domain` stays
/// dependency-light (no `thiserror`), so `Display` is written by hand.
#[derive(Debug, PartialEq, Eq)]
pub enum EmojiImageError {
    Empty,
    /// Actual byte count, then the limit.
    TooLarge(usize, usize),
    UnsupportedFormat,
    Truncated,
    EmptyDimensions,
    TooBig { width: u32, height: u32, max: u32 },
}

impl std::fmt::Display for EmojiImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmojiImageError::Empty => write!(f, "no image data"),
            EmojiImageError::TooLarge(actual, limit) => {
                write!(f, "image is {actual} bytes; the limit is {limit}")
            }
            EmojiImageError::UnsupportedFormat => {
                write!(f, "unrecognized image format (only PNG, GIF, and JPEG are allowed)")
            }
            EmojiImageError::Truncated => write!(f, "image header is truncated"),
            EmojiImageError::EmptyDimensions => write!(f, "image has zero width or height"),
            EmojiImageError::TooBig { width, height, max } => {
                write!(f, "image is {width}×{height}; the limit is {max}×{max}")
            }
        }
    }
}

impl std::error::Error for EmojiImageError {}
