//! The outcome of ingesting a batch of events from a peer.

use crate::auth_error::AuthError;

/// The outcome of ingesting a batch of events from a peer. Authorization never
/// silently drops — every refusal is recorded so the puller can log it and, for
/// [`AuthError::Unowned`](crate::AuthError::Unowned), retry once the missing
/// parent/owner arrives.
#[derive(Debug, Default)]
pub struct Ingested {
    /// How many events were authorized and applied.
    pub applied: u64,
    /// Why each rejected event was refused.
    pub rejected: Vec<AuthError>,
    /// Local apply failures on already-authorized events (store errors).
    pub apply_errors: Vec<String>,
}
