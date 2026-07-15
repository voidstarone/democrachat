//! Validation for uploaded custom-emoji images: format sniffing and the size/
//! dimension rules, kept pure (no image-decoding dependency).

pub mod emoji_image_error;
pub mod emoji_image_format;
pub mod validate_emoji_image;

#[cfg(test)]
mod tests {
    use super::emoji_image_error::EmojiImageError;
    use super::emoji_image_format::EmojiImageFormat;
    use super::validate_emoji_image::{
        validate_emoji_image, MAX_EMOJI_IMAGE_BYTES, MAX_EMOJI_IMAGE_DIMENSION,
    };

    /// A minimal PNG header (magic + IHDR up to the dimensions) declaring `w × h`.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        b.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    /// A minimal GIF header (signature + logical screen descriptor) for `w × h`.
    fn gif(w: u16, h: u16) -> Vec<u8> {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0]); // rest of the descriptor
        b
    }

    #[test]
    fn a_png_at_the_limit_is_accepted() {
        assert_eq!(validate_emoji_image(&png(256, 256)), Ok(EmojiImageFormat::Png));
        assert_eq!(validate_emoji_image(&png(16, 48)), Ok(EmojiImageFormat::Png));
    }

    #[test]
    fn a_gif_is_accepted() {
        assert_eq!(validate_emoji_image(&gif(128, 128)), Ok(EmojiImageFormat::Gif));
    }

    #[test]
    fn an_oversized_image_is_rejected_with_its_dimensions() {
        assert_eq!(
            validate_emoji_image(&png(257, 100)),
            Err(EmojiImageError::TooBig { width: 257, height: 100, max: MAX_EMOJI_IMAGE_DIMENSION })
        );
        assert!(matches!(
            validate_emoji_image(&gif(300, 10)),
            Err(EmojiImageError::TooBig { .. })
        ));
    }

    #[test]
    fn a_zero_dimension_image_is_rejected() {
        assert_eq!(validate_emoji_image(&png(0, 64)), Err(EmojiImageError::EmptyDimensions));
    }

    #[test]
    fn an_unknown_format_is_rejected() {
        assert_eq!(validate_emoji_image(b"\xff\xd8\xff\xe0JFIF"), Err(EmojiImageError::UnsupportedFormat));
        assert_eq!(validate_emoji_image(b"not an image"), Err(EmojiImageError::UnsupportedFormat));
    }

    #[test]
    fn empty_and_oversized_byte_counts_are_rejected() {
        assert_eq!(validate_emoji_image(&[]), Err(EmojiImageError::Empty));
        let huge = vec![0u8; MAX_EMOJI_IMAGE_BYTES + 1];
        assert_eq!(
            validate_emoji_image(&huge),
            Err(EmojiImageError::TooLarge(MAX_EMOJI_IMAGE_BYTES + 1, MAX_EMOJI_IMAGE_BYTES))
        );
    }

    #[test]
    fn a_truncated_header_is_rejected() {
        // PNG magic but no IHDR dimensions.
        let truncated = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert_eq!(validate_emoji_image(&truncated), Err(EmojiImageError::Truncated));
    }
}
