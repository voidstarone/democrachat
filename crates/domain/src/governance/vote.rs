//! A single citizen's ballot on a proposal.

use serde::{Deserialize, Serialize};

use crate::{ProposalId, UserId};

/// One citizen's vote on a proposal — unique per (proposal, voter). The weight it
/// carries is computed at tally time from the server's [`crate::VoteWeighting`],
/// not stored here, so a later weighting change is reflected consistently.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Vote {
    pub proposal_id: ProposalId,
    pub voter: UserId,
    /// `true` = aye, `false` = nay.
    pub is_aye: bool,
}

impl Vote {
    pub fn new(proposal_id: ProposalId, voter: UserId, is_aye: bool) -> Self {
        Self { proposal_id, voter, is_aye }
    }
}
