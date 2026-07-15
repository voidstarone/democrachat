//! Persistence for server invite codes (stored by digest only).

use domain::{Invite, ServerId};

/// Persistence for [`Invite`]s. Codes are looked up by their SHA-256 digest — the
/// raw code is never stored, so this port only ever sees hashes.
pub trait InviteStore: Send + Sync {
    /// Record a freshly minted invite.
    fn add(&self, invite: Invite);
    /// Look up an invite by its code digest, if one exists.
    fn by_hash(&self, code_hash: &str) -> Option<Invite>;
    /// Every invite for a server (live and revoked), for the members' invite list.
    fn list_for_server(&self, server_id: ServerId) -> Vec<Invite>;
    /// Mark the invite with this digest revoked. No-op if absent.
    fn revoke(&self, code_hash: &str);
}
