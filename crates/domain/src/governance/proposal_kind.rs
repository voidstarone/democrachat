//! What a proposal seeks to do.

use serde::{Deserialize, Serialize};

use crate::{
    BallotKind, DecisionClass, FranchiseCriteria, JurySizing, RoleId, RuleId, UserId,
    VoteWeighting, WeightingScope,
};
use std::collections::BTreeSet;

/// What a proposal seeks to do — and, via [`ProposalKind::decision_class`], how
/// hard it should be to pass, and via [`ProposalKind::ballot_kind`], which
/// governance-surface entry it belongs to.
///
/// Note there is deliberately **no** `GrantCitizenship` variant: the franchise
/// is reachable only by meeting criteria ([`crate::evaluate_eligibility`]), never
/// by a vote or a grant. [`ProposalKind::GrantVoteWeight`] weights an *existing*
/// citizen's ballot; it is not a path into the franchise.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ProposalKind {
    /// Remove a message/post, resolve a report. Routine.
    RemoveContent { target: String },
    /// Ban a user from the server.
    Ban { user: UserId },
    /// Time a user out until a given instant (seconds since epoch).
    Timeout { user: UserId, until: i64 },
    /// Recall a leader from office.
    Recall { leader: UserId },
    /// Create a new channel — chat-native layout governance (past Seed, where the
    /// founder no longer provisions channels unilaterally).
    CreateChannel { name: String, topic: String },
    /// Delete a channel by name.
    DeleteChannel { name: String },
    /// Add a community rule.
    AddRule { text: String },
    /// Repeal an existing community rule.
    RemoveRule { rule: RuleId },
    /// Amend the franchise criteria — a constitutional change.
    AmendCriteria { proposed: FranchiseCriteria },
    /// Change how reports are juried (see [`JurySizing`]).
    SetJurySizing { sizing: JurySizing },
    /// Change how the server values its citizens' votes (see [`VoteWeighting`]) —
    /// a constitutional change to the power structure.
    SetVoteWeighting { scheme: VoteWeighting },
    /// Change which decisions vote-weighting applies to (see [`WeightingScope`]).
    SetWeightingScope { scope: WeightingScope },
    /// Grant a specific citizen a vote weight, consulted under the
    /// [`VoteWeighting::ByRole`] scheme. `weight: 1` resets them to an ordinary
    /// citizen. Never a path into the franchise — only ever adjusts an
    /// already-enfranchised citizen's ballot.
    GrantVoteWeight { user: UserId, weight: u32 },
    /// Change the server's governance surface — which ballot kinds are enabled.
    /// The always-on kinds ([`BallotKind::always_on`]) are re-added regardless.
    SetGovernanceSurface { enabled: BTreeSet<BallotKind> },
    /// Create a custom mention role with the given (normalized) name. Grants no
    /// vote or permission — see [`crate::Role`].
    CreateRole { name: String },
    /// Delete a custom role and every assignment to it.
    DeleteRole { role: RoleId },
    /// Add a member to a custom role. Not a path into the franchise: a role
    /// confers nothing but membership in a mention group.
    AssignRole { user: UserId, role: RoleId },
    /// Remove a member from a custom role.
    UnassignRole { user: UserId, role: RoleId },
    /// Enable or disable automatic rehoming of this server across the federation.
    /// `is_disabled: true` opts the server out — if its home node fails it stays
    /// down until that node returns rather than migrating elsewhere (the
    /// community's data-sovereignty choice). See `docs/federation.md` §6.
    SetRehomingPolicy { is_disabled: bool },
    /// Open or close who may mint the server's invite codes (see
    /// [`InvitePolicy`](crate::InvitePolicy)). Closing seals admission-by-link until
    /// a later vote reopens it; never affects the criteria-only franchise.
    SetInvitePolicy { policy: crate::InvitePolicy },
}

impl ProposalKind {
    /// Which governance-surface entry this proposal belongs to. A server can only
    /// open a proposal whose [`BallotKind`] is in its enabled surface.
    pub fn ballot_kind(&self) -> BallotKind {
        match self {
            ProposalKind::RemoveContent { .. } => BallotKind::RemoveContent,
            ProposalKind::Ban { .. } => BallotKind::Ban,
            ProposalKind::Timeout { .. } => BallotKind::Timeout,
            ProposalKind::Recall { .. } => BallotKind::Recall,
            ProposalKind::CreateChannel { .. } => BallotKind::CreateChannel,
            ProposalKind::DeleteChannel { .. } => BallotKind::DeleteChannel,
            ProposalKind::AddRule { .. } => BallotKind::AddRule,
            ProposalKind::RemoveRule { .. } => BallotKind::RemoveRule,
            ProposalKind::AmendCriteria { .. } => BallotKind::AmendCriteria,
            ProposalKind::SetJurySizing { .. } => BallotKind::SetJurySizing,
            ProposalKind::SetVoteWeighting { .. } => BallotKind::SetVoteWeighting,
            ProposalKind::SetWeightingScope { .. } => BallotKind::SetWeightingScope,
            ProposalKind::GrantVoteWeight { .. } => BallotKind::GrantVoteWeight,
            ProposalKind::SetGovernanceSurface { .. } => BallotKind::SetGovernanceSurface,
            ProposalKind::SetRehomingPolicy { .. } => BallotKind::SetRehomingPolicy,
            ProposalKind::SetInvitePolicy { .. } => BallotKind::SetInvitePolicy,
            ProposalKind::CreateRole { .. }
            | ProposalKind::DeleteRole { .. }
            | ProposalKind::AssignRole { .. }
            | ProposalKind::UnassignRole { .. } => BallotKind::ManageRoles,
        }
    }

    pub fn decision_class(&self) -> DecisionClass {
        match self {
            ProposalKind::RemoveContent { .. } => DecisionClass::Moderation,
            ProposalKind::Ban { .. }
            | ProposalKind::Timeout { .. }
            | ProposalKind::Recall { .. }
            | ProposalKind::GrantVoteWeight { .. } => DecisionClass::BanOrRecall,
            // Who holds how much power, who may vote, and where the server's data
            // is allowed to live are all constitutional.
            ProposalKind::AmendCriteria { .. }
            | ProposalKind::SetVoteWeighting { .. }
            | ProposalKind::SetWeightingScope { .. }
            | ProposalKind::SetRehomingPolicy { .. } => DecisionClass::Constitutional,
            ProposalKind::CreateChannel { .. }
            | ProposalKind::DeleteChannel { .. }
            | ProposalKind::AddRule { .. }
            | ProposalKind::RemoveRule { .. }
            | ProposalKind::SetJurySizing { .. }
            | ProposalKind::SetGovernanceSurface { .. }
            | ProposalKind::SetInvitePolicy { .. }
            | ProposalKind::CreateRole { .. }
            | ProposalKind::DeleteRole { .. }
            | ProposalKind::AssignRole { .. }
            | ProposalKind::UnassignRole { .. } => DecisionClass::RuleChange,
        }
    }
}
