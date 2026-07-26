//! The maximum accepted email length.

/// Maximum email length in bytes. 254 is the practical ceiling for a valid
/// email address (RFC 5321 path limit). Enforced by
/// [`validate_email`](crate::validate_email).
pub const MAX_EMAIL_LEN: usize = 254;
