//! The formats a custom emoji image may be uploaded in.

/// The image formats a custom emoji may use — deliberately small: the two formats
/// emoji actually need (PNG for static, GIF for animated), each identifiable from
/// its header with no image-decoding dependency.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmojiImageFormat {
    Png,
    Gif,
}

impl EmojiImageFormat {
    /// The MIME type, for building a `data:` URI the browser can render.
    pub fn mime(self) -> &'static str {
        match self {
            EmojiImageFormat::Png => "image/png",
            EmojiImageFormat::Gif => "image/gif",
        }
    }
}
