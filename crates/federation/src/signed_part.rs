//! The signed portion of a change event.

use serde::{Deserialize, Serialize};

use crate::change_op::ChangeOp;
use crate::event_scope::EventScope;

/// The signed portion of a change event: everything a consumer's decision must
/// depend on. Serialized deterministically (serde emits a struct's fields in
/// declaration order) to produce the exact bytes that are signed and verified.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SignedPart {
    /// The node that produced (and signs) this event.
    pub node: u16,
    /// The ownership epoch the producer held when it emitted this. Fences a
    /// returning old owner: events under a stale epoch are rejected.
    pub epoch: u64,
    /// Monotonic per-node sequence (the producer's outbox id). The consumer's
    /// replay cursor.
    pub seq: u64,
    /// What this change is scoped to — a server, a user's home, or global.
    /// Bound into the signature so an event can't be replayed against a
    /// different scope, and used to route to the right owner + replica.
    pub scope: EventScope,
    /// The entity/store the row belongs to (e.g. `"messages"`, `"votes"`, `"dms"`).
    pub entity: String,
    pub op: ChangeOp,
    /// The row itself, as JSON. For an E2EE entity (DMs, message bodies) this is
    /// an opaque **ciphertext** blob — the envelope signs and transports it
    /// without the producing/consuming node ever reading the plaintext. For a
    /// delete it carries enough of the row to identify what to remove.
    pub payload: serde_json::Value,
}
