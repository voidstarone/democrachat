//! A user's membership record within a server.

use serde::{Deserialize, Serialize};

use crate::{ServerId, Tier, Timestamp, UserId};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Membership {
    pub user_id: UserId,
    pub server_id: ServerId,
    pub joined_at: Timestamp,
    pub tier: Tier,
    /// Under an active sanction (disqualifies from the franchise).
    pub is_sanctioned: bool,
    /// Contributions that existing citizens reacted positively to — endorsement-
    /// weighted, not raw reactions from anyone. The mechanism that produces this
    /// score (and how it resists gaming) is an open question; the domain only
    /// consumes the resulting count.
    pub contribution: i64,
    /// When this member was admitted to the franchise, if ever. Used by the
    /// enfranchisement rate cap (Layer 2) to measure recent admissions.
    pub enfranchised_at: Option<Timestamp>,
    /// Per-member voting weight granted by the server (see
    /// [`crate::ProposalKind::GrantVoteWeight`]). Only consulted under the
    /// [`crate::VoteWeighting::ByRole`] scheme; `1` means an ordinary citizen.
    /// A grant weights an *already-enfranchised* citizen's ballot — it is never a
    /// path to the franchise itself. `#[serde(default = "..")]` defaults older
    /// datasets to an unweighted `1`.
    #[serde(default = "default_granted_weight")]
    pub granted_weight: u32,
    /// This member's personal choice of whether people who join the server *after*
    /// their messages were posted may read them. `true` (the default) keeps history
    /// open to newcomers; `false` hides this member's pre-existing messages from
    /// anyone whose join predates them — a personal, per-server history-privacy
    /// control that never affects members who were already present. Defaults older
    /// datasets to sharing.
    #[serde(default = "default_shares_history")]
    pub shares_history_with_newcomers: bool,
}

fn default_granted_weight() -> u32 {
    1
}

fn default_shares_history() -> bool {
    true
}

impl Membership {
    pub fn joined(user_id: UserId, server_id: ServerId, joined_at: Timestamp) -> Self {
        Self {
            user_id,
            server_id,
            joined_at,
            tier: Tier::Member,
            is_sanctioned: false,
            contribution: 0,
            enfranchised_at: None,
            granted_weight: 1,
            shares_history_with_newcomers: true,
        }
    }

    /// Whether `self` (as the author of a message posted at `posted_at`) lets
    /// `viewer` see it, given when `viewer` joined the server. A viewer who was
    /// already a member when the message was posted always sees it; a later joiner
    /// sees it only if this author still shares history with newcomers.
    pub fn shows_message_to(&self, posted_at: Timestamp, viewer_joined_at: Timestamp) -> bool {
        self.shares_history_with_newcomers || posted_at >= viewer_joined_at
    }

    pub fn is_citizen(&self) -> bool {
        self.tier == Tier::Citizen
    }

    /// Whether this member may currently exercise the franchise: an enfranchised
    /// citizen who is **not** under an active sanction. A sanction disqualifies
    /// from the franchise, so every governance action — casting a ballot, being
    /// empanelled on a jury, voting a verdict — must gate on this, not on the bare
    /// [`is_citizen`](Self::is_citizen) tier (which a convicted member retains
    /// until they re-qualify).
    pub fn is_franchised(&self) -> bool {
        self.is_citizen() && !self.is_sanctioned
    }

    /// Whole days this user has been a member of the server.
    pub fn membership_age_days(&self, now: Timestamp) -> i64 {
        now.days_since(self.joined_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citizen() -> Membership {
        let mut m = Membership::joined(UserId(1), ServerId(1), Timestamp(0));
        m.tier = Tier::Citizen;
        m
    }

    #[test]
    fn a_clean_citizen_is_franchised() {
        assert!(citizen().is_franchised());
    }

    #[test]
    fn a_sanctioned_citizen_keeps_the_tier_but_loses_the_franchise() {
        let mut m = citizen();
        m.is_sanctioned = true;
        assert!(m.is_citizen(), "the Citizen tier is retained until re-qualification");
        assert!(!m.is_franchised(), "but a sanction disqualifies from the franchise");
    }

    #[test]
    fn history_sharing_gates_only_messages_posted_before_a_later_join() {
        let mut author = Membership::joined(UserId(1), ServerId(1), Timestamp(0));
        let viewer_joined = Timestamp(100);
        // Sharing on (default): visible whenever it was posted.
        assert!(author.shows_message_to(Timestamp(50), viewer_joined));
        assert!(author.shows_message_to(Timestamp(150), viewer_joined));
        // Sharing off: only messages from before the viewer joined are hidden.
        author.shares_history_with_newcomers = false;
        assert!(!author.shows_message_to(Timestamp(50), viewer_joined), "posted before the viewer joined → hidden");
        assert!(author.shows_message_to(Timestamp(100), viewer_joined), "posted at the join instant → visible");
        assert!(author.shows_message_to(Timestamp(150), viewer_joined), "posted after the viewer joined → visible");
    }

    #[test]
    fn a_non_citizen_is_never_franchised() {
        let mut m = Membership::joined(UserId(1), ServerId(1), Timestamp(0));
        assert_eq!(m.tier, Tier::Member);
        assert!(!m.is_franchised());
        m.tier = Tier::Guest;
        assert!(!m.is_franchised());
    }
}
