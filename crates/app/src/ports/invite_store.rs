//! Persistence for server invite codes (stored by digest only).

use domain::{Invite, ServerId};
use crate::StoreError;

/// Persistence for [`Invite`]s. Codes are looked up by their SHA-256 digest — the
/// raw code is never stored, so this port only ever sees hashes.
pub trait InviteStore: Send + Sync {
    /// Record a freshly minted invite.
    fn add(&self, invite: Invite) -> Result<(), StoreError>;
    /// Look up an invite by its code digest, if one exists.
    fn by_hash(&self, code_hash: &str) -> Result<Option<Invite>, StoreError>;
    /// Every invite for a server (live and revoked), for the members' invite list.
    fn list_for_server(&self, server_id: ServerId) -> Result<Vec<Invite>, StoreError>;
    /// Mark the invite with this digest revoked. No-op if absent.
    fn revoke(&self, code_hash: &str) -> Result<(), StoreError>;
}
