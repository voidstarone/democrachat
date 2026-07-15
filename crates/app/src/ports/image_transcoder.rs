//! Driven port: normalize an uploaded image before it is stored.
//!
//! Re-encoding an image server-side is a defence: the bytes a viewer's browser
//! finally decodes are freshly produced by *our* encoder, not the uploader's
//! file, so a payload crafted against a browser image decoder never reaches it.
//! It also strips EXIF (camera model, GPS) — a privacy leak — and converts
//! formats browsers can't display (HEIC/HEIF) to JPEG. The real work needs an
//! image codec, so it lives behind this port in an adapter; the app only asks
//! for "the normalized form of this image".

use crate::MediaError;

pub trait ImageTranscoder: Send + Sync {
    /// Normalize image `bytes` declared as `content_type`, returning the possibly
    /// changed `(content_type, bytes)`.
    ///
    /// A still image is decoded and re-encoded — HEIC/HEIF become JPEG, an image
    /// carrying transparency stays PNG, any other still becomes JPEG — which drops
    /// metadata and neutralizes a hostile payload. Animated GIF/WebP pass through
    /// unchanged so their animation survives. Bytes that don't decode as a real
    /// image (or that exceed the decode-bomb guards) are rejected.
    fn normalize(&self, content_type: &str, bytes: &[u8]) -> Result<(String, Vec<u8>), MediaError>;
}
