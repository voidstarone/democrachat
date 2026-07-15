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
    // JPEG: SOI (FFD8) then a run of marker segments. Walk them — without decoding
    // any entropy data — to the Start-Of-Frame that carries the real dimensions.
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let (width, height) = sniff_jpeg(bytes)?;
        return Ok((EmojiImageFormat::Jpeg, width, height));
    }
    Err(EmojiImageError::UnsupportedFormat)
}

/// Read a JPEG's width/height from its Start-Of-Frame marker by scanning the
/// segment chain, never touching the compressed scan data (so no decode work and
/// no "image bomb" surface). A JPEG whose SOF isn't found before the scan begins
/// is treated as an unsupported/corrupt file.
fn sniff_jpeg(bytes: &[u8]) -> Result<(u32, u32), EmojiImageError> {
    let mut i = 2; // past the SOI (FFD8)
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return Err(EmojiImageError::UnsupportedFormat);
        }
        // Skip any 0xFF fill bytes to the marker code.
        let mut m = i + 1;
        while m < bytes.len() && bytes[m] == 0xFF {
            m += 1;
        }
        if m >= bytes.len() {
            return Err(EmojiImageError::Truncated);
        }
        let marker = bytes[m];
        i = m + 1;
        // Standalone markers carry no length: RSTn (D0–D7), SOI (D8), EOI (D9), TEM (01).
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        // Start-Of-Scan: entropy data follows; dimensions must already have appeared.
        if marker == 0xDA {
            break;
        }
        if i + 2 > bytes.len() {
            return Err(EmojiImageError::Truncated);
        }
        let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if len < 2 {
            return Err(EmojiImageError::UnsupportedFormat);
        }
        // The Start-Of-Frame markers (baseline C0, progressive C2, …) carry
        // precision(1) then height(2 BE) then width(2 BE) right after the length.
        let is_sof =
            matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            if i + 7 > bytes.len() {
                return Err(EmojiImageError::Truncated);
            }
            let height = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            return Ok((width, height));
        }
        i += len; // skip this segment (len counts its own 2 length bytes)
    }
    Err(EmojiImageError::UnsupportedFormat)
}
