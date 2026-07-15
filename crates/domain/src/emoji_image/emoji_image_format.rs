//! The formats a custom emoji image may be uploaded in.

/// The image formats a custom emoji may use — deliberately small: PNG for static,
/// GIF for animated, and JPEG for photographic uploads. Each is identifiable from
/// its header, and its dimensions are read straight from the header with no
/// image-decoding dependency.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmojiImageFormat {
    Png,
    Gif,
    Jpeg,
}

impl EmojiImageFormat {
    /// The MIME type, for building a `data:` URI the browser can render.
    pub fn mime(self) -> &'static str {
        match self {
            EmojiImageFormat::Png => "image/png",
            EmojiImageFormat::Gif => "image/gif",
            EmojiImageFormat::Jpeg => "image/jpeg",
        }
    }
}
