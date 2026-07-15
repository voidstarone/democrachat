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
    /// Whether this member holds the **police** power on the server: the ability to
    /// instantly mute another member (and to lift any mute). Appointed and dismissed
    /// only by ballot ([`crate::ProposalKind::AppointPolice`]); never a self-grant.
    /// Orthogonal to the franchise — a police officer need not be a citizen, and
    /// being one confers no vote.
    #[serde(default)]
    pub is_police: bool,
    /// Whether this member is currently **muted** — silenced everywhere but the
    /// `#appeals` channel. A mute never touches the franchise (a muted citizen still
    /// votes); it only gags posting. Imposed instantly by police or by a `Mute`
    /// ballot, lifted by police or by a `LiftMute` ballot.
    #[serde(default)]
    pub is_muted: bool,
    /// The officer who imposed the current mute, if it was a police mute (a
    /// vote-imposed mute has none). Recorded so a vote that lifts the mute can bar
    /// *that* officer from immediately re-muting the same member.
    #[serde(default)]
    pub muted_by: Option<UserId>,
    /// After a vote lifts an officer's mute, that officer is barred from re-muting
    /// this member until this instant — a 24-hour cooldown that stops a lone officer
    /// from overriding the electorate's decision. `None` when no bar is in force.
    #[serde(default)]
    pub remute_blocked_officer: Option<UserId>,
    #[serde(default)]
    pub remute_blocked_until: Option<Timestamp>,
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
            is_police: false,
            is_muted: false,
            muted_by: None,
            remute_blocked_officer: None,
            remute_blocked_until: None,
        }
    }

    /// Whether `officer` may mute this member right now. A police mute is barred
    /// only when a vote recently overturned *this same officer's* mute of *this*
    /// member and the 24-hour cooldown has not yet elapsed.
    pub fn is_remute_blocked_for(&self, officer: UserId, now: Timestamp) -> bool {
        self.remute_blocked_officer == Some(officer)
            && self.remute_blocked_until.is_some_and(|until| now < until)
    }

    /// Impose a mute. `by` is the officer for a police mute, or `None` for a mute
    /// imposed by ballot. Clears any spent re-mute cooldown.
    pub fn mute(&mut self, by: Option<UserId>) {
        self.is_muted = true;
        self.muted_by = by;
    }

    /// Lift the mute. When `by_vote`, any officer who imposed it is barred from
    /// re-muting this member until `cooldown_until` (24 hours on); a police-lifted
    /// mute carries no such bar.
    pub fn unmute(&mut self, by_vote: bool, cooldown_until: Timestamp) {
        if by_vote {
            if let Some(officer) = self.muted_by {
                self.remute_blocked_officer = Some(officer);
                self.remute_blocked_until = Some(cooldown_until);
            }
        }
        self.is_muted = false;
        self.muted_by = None;
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
