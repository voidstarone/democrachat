//! Validate an uploaded emoji image against the platform rules.

use crate::emoji_image::emoji_image_error::EmojiImageError;
use crate::emoji_image::emoji_image_format::EmojiImageFormat;

/// The largest an uploaded emoji image may be, in bytes — it is base64'd into a
/// `data:` URI, stored in the (encrypted-at-rest) snapshot, and replicated with the
/// emoji row, so it must stay small. A 256×256 PNG/GIF is comfortably under this.
pub const MAX_EMOJI_IMAGE_BYTES: usize = 256 * 1024;

/// The largest emoji dimension per side. Emoji render tiny; 256 is generous.
pub const MAX_EMOJI_IMAGE_DIMENSION: u32 = 256;

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Validate emoji image `bytes`: a recognized format (PNG or GIF), within the byte
/// cap, and no larger than [`MAX_EMOJI_IMAGE_DIMENSION`] per side. Returns the
/// detected format so the caller can build the matching `data:` URI.
///
/// Pure and dependency-free: dimensions are read straight from the file header
/// rather than by decoding the whole image, so a hostile upload cannot make the
/// server do decompression work (no "image bomb").
pub fn validate_emoji_image(bytes: &[u8]) -> Result<EmojiImageFormat, EmojiImageError> {
    if bytes.is_empty() {
        return Err(EmojiImageError::Empty);
    }
    if bytes.len() > MAX_EMOJI_IMAGE_BYTES {
        return Err(EmojiImageError::TooLarge(bytes.len(), MAX_EMOJI_IMAGE_BYTES));
    }
    let (format, width, height) = sniff(bytes)?;
    if width == 0 || height == 0 {
        return Err(EmojiImageError::EmptyDimensions);
    }
    if width > MAX_EMOJI_IMAGE_DIMENSION || height > MAX_EMOJI_IMAGE_DIMENSION {
        return Err(EmojiImageError::TooBig { width, height, max: MAX_EMOJI_IMAGE_DIMENSION });
    }
    Ok(format)
}

/// Identify the format and read its declared dimensions from the header.
fn sniff(bytes: &[u8]) -> Result<(EmojiImageFormat, u32, u32), EmojiImageError> {
    if bytes.starts_with(&PNG_MAGIC) {
        // IHDR follows the 8-byte magic + 4-byte length + "IHDR": width is a u32 BE
        // at offset 16, height at 20.
        if bytes.len() < 24 {
            return Err(EmojiImageError::Truncated);
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().expect("checked length"));
        let height = u32::from_be_bytes(bytes[20..24].try_into().expect("checked length"));
        return Ok((EmojiImageFormat::Png, width, height));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        // Logical Screen Descriptor: width is a u16 LE at offset 6, height at 8.
        if bytes.len() < 10 {
            return Err(EmojiImageError::Truncated);
        }
        let width = u16::from_le_bytes(bytes[6..8].try_into().expect("checked length")) as u32;
        let height = u16::from_le_bytes(bytes[8..10].try_into().expect("checked length")) as u32;
        return Ok((EmojiImageFormat::Gif, width, height));
    }
    Err(EmojiImageError::UnsupportedFormat)
}
