//! Why a replicated event was refused.

use crate::fed_error::FedError;

/// Why [`authorize`](crate::authorize::authorize) refused an event. Authorization
/// never silently drops — a caller records the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The signature/body did not check out (forged, tampered, malformed).
    Fed(FedError),
    /// The claimed producing node has no published key — we can't verify it.
    UnknownNode,
    /// Correctly signed, but the signer does not own the scope the payload
    /// actually belongs to (the anti-forgery check).
    NotOwner,
    /// Signed by the rightful owner, but under an ownership epoch older than the
    /// scope's current one — a fenced, returning old owner.
    StaleEpoch,
    /// The scope is currently unowned, or a parent needed to place the row has not
    /// replicated yet — the puller should retry.
    Unowned,
    /// The event's entity/payload could not be placed in any scope — not a row we
    /// replicate. Refused outright.
    ScopeMismatch,
    /// The control plane could not be consulted.
    Registry(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Fed(e) => write!(f, "{e}"),
            AuthError::UnknownNode => write!(f, "unknown producing node (no published key)"),
            AuthError::NotOwner => write!(f, "signer does not own the row's scope"),
            AuthError::StaleEpoch => write!(f, "event minted under a stale ownership epoch"),
            AuthError::Unowned => write!(f, "scope unowned or parent not yet replicated"),
            AuthError::ScopeMismatch => write!(f, "event could not be placed in any scope"),
            AuthError::Registry(e) => write!(f, "control-plane error: {e}"),
        }
    }
}

impl std::error::Error for AuthError {}
