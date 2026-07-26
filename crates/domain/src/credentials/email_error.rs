//! Why a proposed email address was rejected.

/// Why a proposed email failed [`validate_email`](crate::validate_email).
///
/// Hand-written `Display`/`Error` (no `thiserror`) so `domain` stays dependency-
/// light — it depends only on `serde`. This is a deliberately conservative
/// syntactic check (there is no way to prove an address deliverable without
/// sending to it); the real proof of ownership is the verification email.
#[derive(Debug, PartialEq, Eq)]
pub enum EmailError {
    /// Empty or whitespace-only.
    Empty,
    /// Longer than [`MAX_EMAIL_LEN`](crate::MAX_EMAIL_LEN) bytes.
    TooLong,
    /// Not a plausible `local@domain` address (missing/duplicate `@`, empty
    /// local or domain part, no dot in the domain, or contains whitespace).
    Malformed,
}

impl std::fmt::Display for EmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmailError::Empty => write!(f, "email must not be empty"),
            EmailError::TooLong => {
                write!(f, "email must be at most {} characters", crate::MAX_EMAIL_LEN)
            }
            EmailError::Malformed => write!(f, "email is not a valid address"),
        }
    }
}

impl std::error::Error for EmailError {}
