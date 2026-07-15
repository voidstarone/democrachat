//! A re-encoding [`ImageTranscoder`] — the image-normalization tier.
//!
//! Every uploaded image is decoded and re-encoded before it is stored, so the
//! bytes a viewer's browser eventually decodes are produced by *our* encoder,
//! not the uploader's file. That neutralizes a payload crafted against a browser
//! image decoder, and drops EXIF (camera, GPS) as a side effect. Formats a
//! browser can't display — **HEIC/HEIF** — are converted to JPEG.
//!
//! Policy (per the product decision):
//! - **HEIC / HEIF** → decode via libheif, re-encode JPEG.
//! - **Animated GIF / animated WebP** → passed through untouched, so the
//!   animation survives (still bounded by the caller's size + markup checks).
//! - **Any other still** (JPEG, PNG, static WebP/GIF, BMP, TIFF) → decoded and
//!   re-encoded: PNG if it carries an alpha channel (keep transparency), else
//!   JPEG.
//! - Bytes that don't decode as a real image, or that trip a decode-bomb guard,
//!   are rejected.
//!
//! The pure-Rust `image` crate handles the common formats; only HEIC/HEIF pulls
//! in the system `libheif` C library.

use std::io::Cursor;

use app::{ImageTranscoder, MediaError};
use image::{ImageReader, Limits};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma, SecurityLimits};

/// Largest allowed dimension per side, and total pixels — a decode-bomb guard so
/// a tiny hostile file can't force a huge allocation. 50 MP covers current phone
/// cameras with headroom.
const MAX_DIMENSION: u32 = 12_000;
const MAX_PIXELS: u64 = 50_000_000;

/// JPEG quality for re-encoded stills — visually lossless for photos at a sane
/// size.
const JPEG_QUALITY: u8 = 85;

/// Re-encodes uploaded images to strip metadata/exploits and convert HEIC/HEIF
/// to JPEG. Stateless.
pub struct ReencodingTranscoder;

impl ReencodingTranscoder {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReencodingTranscoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageTranscoder for ReencodingTranscoder {
    fn normalize(&self, content_type: &str, bytes: &[u8]) -> Result<(String, Vec<u8>), MediaError> {
        // HEIC/HEIF: browsers can't render it, so always convert to JPEG. Detected
        // from the file's `ftyp` brand rather than the (spoofable) declared type.
        if is_heif(bytes) {
            return heif_to_jpeg(bytes);
        }
        // Preserve animation: an animated GIF/WebP is stored as-is.
        if is_animated_gif(bytes) || is_animated_webp(bytes) {
            return Ok((content_type.to_string(), bytes.to_vec()));
        }
        // Everything else is treated as a still: decode + re-encode.
        reencode_still(bytes)
    }
}

/// True if `bytes` is an ISO-BMFF HEIF/HEIC file — a `ftyp` box at offset 4 whose
/// major or a compatible brand is one libheif handles.
fn is_heif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    // The declared box size (bytes 0..4) bounds how many compatible-brand slots
    // follow the major brand; scanning the header in 4-byte brands is enough.
    const BRANDS: [&[u8]; 8] = [
        b"heic", b"heix", b"heim", b"heis", b"hevc", b"mif1", b"msf1", b"heif",
    ];
    let header = &bytes[8..bytes.len().min(64)];
    header.chunks_exact(4).any(|b| BRANDS.contains(&b))
}

/// Convert a HEIC/HEIF blob to JPEG. Guards the decoded dimensions before
/// allocating, so a small file declaring a huge canvas is rejected, not decoded.
fn heif_to_jpeg(bytes: &[u8]) -> Result<(String, Vec<u8>), MediaError> {
    let mut ctx = HeifContext::read_from_bytes(bytes).map_err(|_| MediaError::Undecodable)?;
    // libheif's own decode-bomb guard. Its default is very small (it rejects even
    // tile-aligned small images), so set it to our own pixel ceiling — which both
    // lifts the too-tight default and enforces the bomb guard inside the decoder.
    let mut limits = SecurityLimits::new();
    limits.set_max_image_size_pixels(MAX_PIXELS);
    ctx.set_security_limits(&limits).map_err(|_| MediaError::Undecodable)?;
    let handle = ctx.primary_image_handle().map_err(|_| MediaError::Undecodable)?;
    let (w, h) = (handle.width(), handle.height());
    if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION || (w as u64 * h as u64) > MAX_PIXELS
    {
        return Err(MediaError::Undecodable);
    }
    let lib = LibHeif::new();
    // Interleaved 24-bit RGB — HEIC photos have no alpha, and the JPEG target
    // couldn't carry it anyway.
    let decoded = lib
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
        .map_err(|_| MediaError::Undecodable)?;
    let planes = decoded.planes();
    let plane = planes.interleaved.ok_or(MediaError::Undecodable)?;
    let (pw, ph, stride) = (plane.width, plane.height, plane.stride);
    let row_bytes = pw as usize * 3;
    if stride < row_bytes {
        return Err(MediaError::Undecodable);
    }
    // Copy the padded (stride ≥ width*3) plane into a tight RGB buffer.
    let mut buf = Vec::with_capacity(row_bytes * ph as usize);
    for y in 0..ph as usize {
        buf.extend_from_slice(&plane.data[y * stride..y * stride + row_bytes]);
    }
    let rgb = image::RgbImage::from_raw(pw, ph, buf).ok_or(MediaError::Undecodable)?;
    encode_jpeg(&rgb)
}

/// Decode a still with dimension/allocation limits, then re-encode it: PNG if it
/// has an alpha channel (keep transparency), otherwise JPEG.
fn reencode_still(bytes: &[u8]) -> Result<(String, Vec<u8>), MediaError> {
    // Cheap dimension read first, so a bomb is rejected before a full decode.
    let dims_reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| MediaError::Undecodable)?;
    let (w, h) = dims_reader.into_dimensions().map_err(|_| MediaError::Undecodable)?;
    if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION || (w as u64 * h as u64) > MAX_PIXELS
    {
        return Err(MediaError::Undecodable);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| MediaError::Undecodable)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| MediaError::Undecodable)?;

    if img.color().has_alpha() {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|_| MediaError::Undecodable)?;
        Ok(("image/png".to_string(), out))
    } else {
        encode_jpeg(&img.to_rgb8())
    }
}

/// Encode an RGB buffer as JPEG at [`JPEG_QUALITY`].
fn encode_jpeg(rgb: &image::RgbImage) -> Result<(String, Vec<u8>), MediaError> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(rgb)
        .map_err(|_| MediaError::Undecodable)?;
    Ok(("image/jpeg".to_string(), out))
}

/// A GIF is treated as animated if it carries the NETSCAPE looping application
/// extension — the marker every animated GIF writes. A rare multi-frame GIF
/// without it would be re-encoded to its first frame, which is acceptable.
fn is_animated_gif(bytes: &[u8]) -> bool {
    (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && find(bytes, b"NETSCAPE2.0")
}

/// A WebP is animated if its RIFF container declares an `ANIM` chunk (which sits
/// in the first few dozen bytes, right after the `VP8X` header).
fn is_animated_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
        && find(&bytes[..bytes.len().min(64)], b"ANIM")
}

/// Naïve substring search over a byte slice.
fn find(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    //! Unit tests over the private format sniffers — the spoof-resistant part of the
    //! tier (detection reads the bytes, never the declared content type). The
    //! black-box `normalize` behaviour is covered in `tests/transcode.rs`.

    use super::*;

    /// HEIF detection reads the `ftyp` brand, not the declared type: a HEIC header is
    /// recognized, ordinary magic (JPEG) is not.
    #[test]
    fn heif_is_detected_from_the_ftyp_brand() {
        let mut heic = vec![0, 0, 0, 0x18];
        heic.extend_from_slice(b"ftyp");
        heic.extend_from_slice(b"heic"); // major brand
        heic.extend_from_slice(&[0u8; 8]);
        assert!(is_heif(&heic), "a heic ftyp brand is recognized");
        assert!(!is_heif(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0]), "JPEG is not HEIF");
        assert!(!is_heif(b"too short"), "a short buffer is not HEIF");
    }

    /// The animation sniffers key on the container markers, not the declared type: a
    /// GIF89a with the NETSCAPE2.0 loop marker is animated; a bare header is not.
    #[test]
    fn animation_is_sniffed_from_container_markers() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(b"\x21\xFF\x0BNETSCAPE2.0");
        assert!(is_animated_gif(&gif));
        assert!(!is_animated_gif(b"GIF89a plain still without a loop marker"));
        assert!(!is_animated_gif(&png_marker()), "a PNG is not a GIF");
    }

    fn png_marker() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n".to_vec()
    }
}
