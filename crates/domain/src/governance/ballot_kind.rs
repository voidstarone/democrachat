//! The governance surface: which *kinds* of decision a server puts to a vote.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A discriminant over the votable actions — the payload-free "kind" of a
/// [`ProposalKind`](crate::ProposalKind).
///
/// Each server enables a **subset** of these: its *governance surface*. One server
/// might only put bans to a vote; a large server might govern its whole rulebook
/// and jury policy. What a server has *not* enabled simply cannot be proposed
/// there. (Custom emoji are *not* governed here — they are curated by continuous
/// per-emoji voting, see [`crate::emoji_ranking`].)
///
/// Two kinds are structural and always enabled, so a server can never wall itself
/// off from self-government: [`BallotKind::AmendCriteria`] (change who may vote)
/// and — implicitly via the same surface — the ability to change the surface
/// itself lives with the founder/citizens through [`BallotKind::AddRule`]-class
/// governance. See [`BallotKind::always_on`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum BallotKind {
    /// Remove a message/post, resolve a report.
    RemoveContent,
    /// Ban a user from the server.
    Ban,
    /// Time a user out for a bounded duration.
    Timeout,
    /// Mute a member (silence them everywhere but the appeals channel).
    Mute,
    /// Lift a member's mute.
    LiftMute,
    /// Appoint or dismiss a **police** officer (the instant-mute power).
    Policing,
    /// Recall a leader from office.
    Recall,
    /// Create a channel.
    CreateChannel,
    /// Delete a channel.
    DeleteChannel,
    /// Add a community rule.
    AddRule,
    /// Repeal a community rule.
    RemoveRule,
    /// Amend the franchise criteria (who may become a citizen). Structural.
    AmendCriteria,
    /// Change how reports are juried.
    SetJurySizing,
    /// Change how the server weights its citizens' votes.
    SetVoteWeighting,
    /// Change which decisions vote-weighting applies to.
    SetWeightingScope,
    /// Grant a specific citizen a vote weight (only under `ByRole` weighting).
    GrantVoteWeight,
    /// Change the server's governance surface itself — which of these ballot kinds
    /// are enabled.
    SetGovernanceSurface,
    /// Create, delete, and populate custom mention roles.
    ManageRoles,
    /// Enable or disable automatic rehoming of this server across the federation —
    /// the community's data-sovereignty choice.
    SetRehomingPolicy,
    /// Open or close the server's invite policy — whether any member may mint invite
    /// codes, or admission is sealed until reopened by vote.
    SetInvitePolicy,
}

impl BallotKind {
    /// Ballot kinds that are **always** enabled and cannot be removed from a
    /// server's surface — the guardrails that keep a server self-governing. A server
    /// must always be able to change who may vote, and to change its own surface.
    pub fn always_on() -> [BallotKind; 2] {
        [BallotKind::AmendCriteria, BallotKind::SetGovernanceSurface]
    }

    /// Every ballot kind the platform knows, in a stable order. The catalogue a
    /// server picks from when it edits its governance surface.
    pub fn all() -> [BallotKind; 20] {
        use BallotKind::*;
        [
            RemoveContent,
            Ban,
            Timeout,
            Mute,
            LiftMute,
            Policing,
            Recall,
            CreateChannel,
            DeleteChannel,
            AddRule,
            RemoveRule,
            AmendCriteria,
            SetJurySizing,
            SetVoteWeighting,
            SetWeightingScope,
            GrantVoteWeight,
            SetGovernanceSurface,
            ManageRoles,
            SetRehomingPolicy,
            SetInvitePolicy,
        ]
    }

    /// The kind's canonical wire name — the single string form crossing the
    /// web↔domain boundary. Deliberately explicit (not `{:?}`): the `Debug`
    /// rendering is not a stability contract, and this is what both the serialized
    /// surface and [`from_name`](Self::name) agree on. The strings match the
    /// derived serde representation (pinned by a test), so a stored surface and a
    /// client's surface parse identically.
    pub const fn name(self) -> &'static str {
        use BallotKind::*;
        match self {
            RemoveContent => "RemoveContent",
            Ban => "Ban",
            Timeout => "Timeout",
            Mute => "Mute",
            LiftMute => "LiftMute",
            Policing => "Policing",
            Recall => "Recall",
            CreateChannel => "CreateChannel",
            DeleteChannel => "DeleteChannel",
            AddRule => "AddRule",
            RemoveRule => "RemoveRule",
            AmendCriteria => "AmendCriteria",
            SetJurySizing => "SetJurySizing",
            SetVoteWeighting => "SetVoteWeighting",
            SetWeightingScope => "SetWeightingScope",
            GrantVoteWeight => "GrantVoteWeight",
            SetGovernanceSurface => "SetGovernanceSurface",
            ManageRoles => "ManageRoles",
            SetRehomingPolicy => "SetRehomingPolicy",
            SetInvitePolicy => "SetInvitePolicy",
        }
    }

    /// Parse a canonical [`name`](Self::name) back into a kind, ignoring any name
    /// the platform no longer knows.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().into_iter().find(|k| k.name() == name)
    }

    /// The platform-default surface a new server starts from: everyday moderation
    /// and rulebook governance, plus the always-on structural kinds. Emoji and
    /// vote-weighting governance are opt-in — a server turns them on if it wants to
    /// decide those collectively.
    pub fn platform_default_surface() -> BTreeSet<BallotKind> {
        use BallotKind::*;
        let mut s: BTreeSet<BallotKind> = [
            RemoveContent,
            Ban,
            Timeout,
            Mute,
            LiftMute,
            Policing,
            Recall,
            CreateChannel,
            DeleteChannel,
            AddRule,
            RemoveRule,
            SetJurySizing,
            ManageRoles,
            SetRehomingPolicy,
            SetInvitePolicy,
        ]
        .into_iter()
        .collect();
        s.extend(Self::always_on());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web layer serialises the surface with [`BallotKind::name`] and parses it
    /// back with `from_name`; every kind must survive that round-trip.
    #[test]
    fn every_kind_round_trips_through_its_name() {
        for k in BallotKind::all() {
            assert_eq!(BallotKind::from_name(k.name()), Some(k));
        }
        assert_eq!(BallotKind::from_name("NotAKind"), None);
    }

    /// The canonical name must equal the derived serde representation. The stored
    /// surface is (de)serialized through serde ([`Server::deserialize_surface`])
    /// while the web surface goes through `name`/`from_name`; if the two forms ever
    /// diverged (a `#[serde(rename)]`, a variant rename), a stored server and a
    /// client would disagree about the same kind. This pins them together.
    #[test]
    fn the_name_matches_the_serde_representation() {
        for k in BallotKind::all() {
            let serde_repr = serde_json::to_string(&k).unwrap();
            assert_eq!(serde_repr, format!("\"{}\"", k.name()), "serde and name must agree for {k:?}");
        }
    }
}
