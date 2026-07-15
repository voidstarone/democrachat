//! Errors from parsing keys/signatures or verifying an event.

/// Errors from parsing keys/signatures or verifying a [`ChangeEvent`](crate::ChangeEvent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FedError {
    /// A hex string was malformed or the wrong length.
    BadHex(&'static str),
    /// A verifying key's bytes were not a valid Ed25519 point.
    BadKey,
    /// The signature did not verify against the given key.
    BadSignature,
    /// The signed body was not valid JSON for a `SignedPart`.
    BadBody,
}

impl std::fmt::Display for FedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FedError::BadHex(what) => write!(f, "malformed hex for {what}"),
            FedError::BadKey => write!(f, "invalid verifying key"),
            FedError::BadSignature => write!(f, "signature verification failed"),
            FedError::BadBody => write!(f, "malformed signed body"),
        }
    }
}

impl std::error::Error for FedError {}
