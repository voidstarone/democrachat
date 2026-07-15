//! A server invite: a shareable code that admits its bearer as a **member**.

use serde::{Deserialize, Serialize};

use crate::{ServerId, Timestamp, UserId};

/// A reusable, revocable invite to one server.
///
/// The raw code is never stored — only [`Invite::code_hash`], the SHA-256 digest of
/// the code the minter shares out of band. A leaked snapshot therefore yields no
/// working invites, exactly as password *hashes* protect passwords. Redeeming an
/// invite grants **membership only**; citizenship stays criteria-only, so an invite
/// is never a path into the franchise.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Invite {
    /// SHA-256 (lower-hex) digest of the shared code. Lookups hash the presented
    /// code and match on this.
    pub code_hash: String,
    /// The server this invite admits the bearer to.
    pub server_id: ServerId,
    /// The member who minted it.
    pub created_by: UserId,
    /// When it was minted.
    pub created_at: Timestamp,
    /// Whether it has been revoked. A revoked invite no longer admits anyone; it is
    /// kept (not deleted) so the same code can never be silently re-minted into a
    /// live one.
    pub is_revoked: bool,
}

impl Invite {
    /// Mint an active invite for `server_id`, storing only the code's digest.
    pub fn new(
        code_hash: impl Into<String>,
        server_id: ServerId,
        created_by: UserId,
        created_at: Timestamp,
    ) -> Self {
        Self {
            code_hash: code_hash.into(),
            server_id,
            created_by,
            created_at,
            is_revoked: false,
        }
    }

    /// Whether this invite currently admits a bearer (exists and not revoked).
    pub fn is_live(&self) -> bool {
        !self.is_revoked
    }
}
