//! An error from a persistence port — the seam where the durable store can fail
//! independently of any domain rule. The in-memory store never returns these (it
//! is infallible); a real backend (Postgres in production) does, and the service
//! layer propagates them so a caller can tell "the rule said no" from "the database
//! is down".

use std::fmt;

/// A failure of the persistence layer itself, distinct from any domain-rule
/// rejection. Variants mirror the ways a pooled SQL backend fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store is unreachable — connection refused, pool exhausted, node down.
    Unavailable(String),
    /// The store accepted the request but did not answer within the deadline.
    Timeout,
    /// A write lost a race and must be retried (serialization failure / deadlock
    /// abort). Surfaced so a caller can retry rather than treat it as a hard no.
    Conflict,
    /// The store answered, but a row could not be decoded into its domain type —
    /// a schema/data-corruption signal, never a normal outcome.
    Corrupt(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Unavailable(why) => write!(f, "the store is unavailable: {why}"),
            StoreError::Timeout => write!(f, "the store timed out"),
            StoreError::Conflict => write!(f, "the write conflicted; retry"),
            StoreError::Corrupt(why) => write!(f, "the store returned corrupt data: {why}"),
        }
    }
}

impl std::error::Error for StoreError {}
