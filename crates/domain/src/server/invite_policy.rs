//! Who may mint a server's invite codes.

use serde::{Deserialize, Serialize};

/// A server's invite policy — the governed answer to "who can let new people in?".
///
/// **Open** (the default) is the bootstrap posture: any member may mint an invite
/// code, so a young server grows virally toward its first few voters. Once a server
/// is officially founded (reaches [`Phase::Chartering`](crate::Phase), 5 voters)
/// its citizens may vote to **close** the door — after which no member may mint a
/// code and admission is decided by the community, not by whoever holds a link.
/// Reopening is likewise a vote. An invite never grants the franchise regardless of
/// policy; it only grants membership.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum InvitePolicy {
    /// Any member may mint invite codes. The default and the Seed-phase posture.
    #[default]
    Open,
    /// Minting is sealed — no member may create or redeem an invite code. The
    /// community voted to close admission; reopening is another vote.
    Closed,
}

impl InvitePolicy {
    /// Whether an ordinary member may mint (or redeem) an invite code under this
    /// policy right now.
    pub fn is_open(self) -> bool {
        matches!(self, InvitePolicy::Open)
    }
}
