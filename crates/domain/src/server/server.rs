//! The founded server entity.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    BallotKind, FranchiseCriteria, InvitePolicy, ServerId, JurySizing, Tags, Timestamp, UserId,
    VoteWeighting, WeightingScope,
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: ServerId,
    pub slug: String,
    pub name: String,
    pub founder_id: UserId,
    pub created_at: Timestamp,
    /// The server's current franchise constitution (amendable only by
    /// constitutional vote — never in Seed).
    pub criteria: FranchiseCriteria,
    /// The server's **governance surface**: which kinds of decision it puts to a
    /// vote. Different servers govern different things. Always contains the
    /// always-on kinds ([`BallotKind::always_on`]).
    #[serde(
        default = "BallotKind::platform_default_surface",
        deserialize_with = "deserialize_surface"
    )]
    pub enabled_ballots: BTreeSet<BallotKind>,
    /// How this server sizes the jury that judges a report (amendable by vote).
    #[serde(default)]
    pub jury_sizing: JurySizing,
    /// How this server values its citizens' votes (amendable by vote). Defaults to
    /// one-citizen-one-vote.
    #[serde(default)]
    pub vote_weighting: VoteWeighting,
    /// Which decisions the [`Server::vote_weighting`] scheme applies to.
    #[serde(default)]
    pub weighting_scope: WeightingScope,
    /// Whether this server's citizens have voted to **disable automatic
    /// rehoming**: if their home node fails, the server stays down until it
    /// returns rather than migrating onto a node the community did not choose
    /// (sovereignty over availability). Default `false` (rehoming on). This is the
    /// authoritative policy; a federated node syncs it to the control plane's
    /// `set_rehoming` (see `docs/federation.md` §6).
    #[serde(default)]
    pub is_rehoming_disabled: bool,
    /// Whether this server is **private**: hidden from the public browse directory
    /// and joinable only with an invite code. Public servers (the default) are
    /// listed and freely joinable. Amendable by the founder in Seed, by vote after.
    #[serde(default)]
    pub is_private: bool,
    /// Who may currently mint invite codes here (see [`InvitePolicy`]). Defaults to
    /// `Open` (any member) so a young server can grow; citizens may vote it `Closed`
    /// once the server is officially founded.
    #[serde(default)]
    pub invite_policy: InvitePolicy,
    /// Free-form discovery tags for this server. Empty by default; `#[serde(default)]`
    /// keeps pre-tags datasets loadable.
    #[serde(default)]
    pub tags: Tags,
}

/// Deserialize the governance surface, **tolerating retired ballot kinds**. A
/// snapshot written by an older release may list a kind a later release removed —
/// e.g. `AddEmoji`/`RemoveEmoji`, dropped once emoji curation moved to continuous
/// per-emoji voting ([`crate::emoji_ranking`]). Such a kind can no longer be
/// proposed, so we drop it rather than fail the whole snapshot load and brick the
/// node. Real corruption elsewhere still errors — only unknown *ballot-kind names*
/// in this set are skipped. The always-on kinds are re-added so a loaded server can
/// never end up unable to govern itself.
fn deserialize_surface<'de, D>(de: D) -> Result<BTreeSet<BallotKind>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::IntoDeserializer;
    let names = Vec::<String>::deserialize(de)?;
    let mut enabled: BTreeSet<BallotKind> = names
        .into_iter()
        .filter_map(|name| {
            BallotKind::deserialize(
                <String as IntoDeserializer<'de, D::Error>>::into_deserializer(name),
            )
            .ok()
        })
        .collect();
    enabled.extend(BallotKind::always_on());
    Ok(enabled)
}

impl Server {
    pub fn new(
        id: ServerId,
        slug: impl Into<String>,
        name: impl Into<String>,
        founder_id: UserId,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            slug: slug.into(),
            name: name.into(),
            founder_id,
            created_at,
            criteria: FranchiseCriteria::platform_default(),
            enabled_ballots: BallotKind::platform_default_surface(),
            jury_sizing: JurySizing::default(),
            vote_weighting: VoteWeighting::default(),
            weighting_scope: WeightingScope::default(),
            is_rehoming_disabled: false,
            is_private: false,
            invite_policy: InvitePolicy::default(),
            tags: Tags::default(),
        }
    }

    /// Whether a member may currently mint an invite code here — true only while the
    /// [`InvitePolicy`] is `Open`. A closed server admits no one by link until its
    /// citizens vote it back open.
    pub fn allows_member_invites(&self) -> bool {
        self.invite_policy.is_open()
    }

    /// Whether a proposal of this kind may be opened here — i.e. the server has
    /// enabled that entry on its governance surface.
    pub fn governs(&self, kind: BallotKind) -> bool {
        self.enabled_ballots.contains(&kind)
    }

    /// Replace the governance surface, re-adding the always-on kinds so a server
    /// can never wall itself off from self-government.
    pub fn set_governance_surface(&mut self, mut enabled: BTreeSet<BallotKind>) {
        enabled.extend(BallotKind::always_on());
        self.enabled_ballots = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(ServerId(1), "gaming", "Gamers", UserId(1), Timestamp(0))
    }

    #[test]
    fn default_surface_governs_moderation_but_not_vote_weighting() {
        let g = server();
        assert!(g.governs(BallotKind::Ban));
        assert!(g.governs(BallotKind::AddRule));
        assert!(!g.governs(BallotKind::SetVoteWeighting), "vote-weighting governance is opt-in");
    }

    #[test]
    fn a_server_can_opt_into_vote_weighting_governance() {
        let mut g = server();
        let mut surface = g.enabled_ballots.clone();
        surface.insert(BallotKind::SetVoteWeighting);
        surface.insert(BallotKind::SetWeightingScope);
        g.set_governance_surface(surface);
        assert!(g.governs(BallotKind::SetVoteWeighting));
    }

    #[test]
    fn surface_load_tolerates_retired_ballot_kinds() {
        use serde::de::value::{Error, SeqDeserializer};
        // A snapshot written by an older release: its surface still lists the retired
        // `AddEmoji`/`RemoveEmoji` kinds. Loading must not fail (a retired kind just
        // can't be proposed) — it drops them and keeps the known ones.
        let stored = ["Ban", "AddEmoji", "AddRule", "RemoveEmoji"];
        let de = SeqDeserializer::<_, Error>::new(stored.iter().map(|s| s.to_string()));
        let surface = deserialize_surface(de).expect("retired kinds must not fail the load");

        assert!(surface.contains(&BallotKind::Ban));
        assert!(surface.contains(&BallotKind::AddRule));
        // Always-on kinds are re-added regardless of what was stored.
        assert!(surface.contains(&BallotKind::AmendCriteria));
        assert!(surface.contains(&BallotKind::SetGovernanceSurface));
        // Ban + AddRule + the two always-on kinds; the two emoji entries were dropped.
        assert_eq!(surface.len(), 4);
    }

    #[test]
    fn always_on_kinds_survive_a_surface_reset() {
        let mut g = server();
        // Try to strip everything, including the structural kinds.
        g.set_governance_surface(BTreeSet::new());
        assert!(g.governs(BallotKind::AmendCriteria), "franchise amendment is always on");
        assert!(g.governs(BallotKind::SetGovernanceSurface), "surface control is always on");
        assert!(!g.governs(BallotKind::Ban));
    }
}
