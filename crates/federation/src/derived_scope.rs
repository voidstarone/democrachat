//! The authoritative scope a change event belongs to, derived from its payload.

use crate::ownership::owned_scope::OwnedScope;

/// The scope a row **actually** belongs to, derived from the signed *payload*
/// (never the envelope's self-declared scope, which a malicious signer controls).
/// This is what [`authorize`](crate::authorize::authorize) binds the ownership
/// check to, so a node that owns one scope cannot stamp an event whose row belongs
/// to another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DerivedScope {
    /// The row carries its scope id directly (server_id, or a user id for the
    /// social graph).
    Owned(OwnedScope),
    /// A vote — its server is that of its parent proposal, resolved locally.
    ViaProposal(u64),
    /// A reaction — its server is that of the message it hangs on.
    ViaMessage(u64),
    /// The entity/payload could not be placed. Refused.
    Indeterminate,
}
