//! The trivial [`ImageTranscoder`]: hand every image back unchanged.
//!
//! It is the default the in-memory store bundle carries, so tests and any driver
//! that doesn't wire a real codec keep working (and stay codec-free). The
//! composition root overrides it with the re-encoding adapter for production.

use crate::{ImageTranscoder, MediaError};

/// An [`ImageTranscoder`] that stores images as uploaded — no re-encoding.
pub struct PassthroughTranscoder;

impl ImageTranscoder for PassthroughTranscoder {
    fn normalize(&self, content_type: &str, bytes: &[u8]) -> Result<(String, Vec<u8>), MediaError> {
        Ok((content_type.to_string(), bytes.to_vec()))
    }
}
