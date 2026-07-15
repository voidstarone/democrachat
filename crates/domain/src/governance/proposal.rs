//! A governance proposal and its close/apply lifecycle.

use serde::{Deserialize, Serialize};

use crate::{
    decide, threshold_for, DecisionClass, ServerId, Phase, ProposalId, ProposalKind, ProposalStatus,
    Tally, Timestamp, UserId, RECALL_WINDOW_DAYS,
};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    pub server_id: ServerId,
    pub proposer: UserId,
    pub kind: ProposalKind,
    pub opened_at: Timestamp,
    pub closes_at: Timestamp,
    pub status: ProposalStatus,
    /// Whether the effects of a *passed* proposal have already been applied. The
    /// application applies them once, after the timelock matures, and sets this —
    /// so re-invoking the close/apply path is idempotent. `#[serde(default)]`
    /// reads older records as not-yet-applied. Meaningless while `status` is `Open`.
    #[serde(default)]
    pub is_applied: bool,
}

impl Proposal {
    pub fn new(
        id: ProposalId,
        server_id: ServerId,
        proposer: UserId,
        kind: ProposalKind,
        opened_at: Timestamp,
        closes_at: Timestamp,
    ) -> Self {
        Self {
            id,
            server_id,
            proposer,
            kind,
            opened_at,
            closes_at,
            status: ProposalStatus::Open,
            is_applied: false,
        }
    }

    /// Layers 3 & 4 together — close the proposal at `closed_at`: apply the
    /// phase-appropriate threshold, and on a passing constitutional change apply
    /// the timelock so the result only becomes effective after the recall window.
    ///
    /// `effective_at` is measured from the actual close moment (`closed_at`), so a
    /// proposal closed early takes effect immediately (or, if constitutional, a
    /// recall window later). A `None` threshold (decision not permitted in this
    /// phase) is treated as a failure.
    pub fn close(
        &mut self,
        tally: Tally,
        established_citizens: u64,
        phase: Phase,
        closed_at: Timestamp,
    ) -> ProposalStatus {
        let class = self.kind.decision_class();
        let status = match threshold_for(class, phase) {
            None => ProposalStatus::Failed,
            Some(threshold) => {
                if decide(tally, established_citizens, threshold).did_pass {
                    let effective_at = if class == DecisionClass::Constitutional {
                        closed_at.plus_days(RECALL_WINDOW_DAYS)
                    } else {
                        closed_at
                    };
                    ProposalStatus::Passed { effective_at }
                } else {
                    ProposalStatus::Failed
                }
            }
        };
        self.status = status;
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FranchiseCriteria;

    const DAY: i64 = Timestamp::SECONDS_PER_DAY;

    fn amend_proposal() -> Proposal {
        Proposal::new(
            ProposalId(1),
            ServerId(1),
            UserId(1),
            ProposalKind::AmendCriteria {
                proposed: FranchiseCriteria::platform_default(),
            },
            Timestamp(0),
            Timestamp(3 * DAY),
        )
    }

    #[test]
    fn passing_amendment_is_timelocked_past_the_recall_window() {
        let mut p = amend_proposal();
        let closed_at = Timestamp(3 * DAY);
        let status = p.close(Tally { aye: 80, nay: 20 }, 100, Phase::Sovereign, closed_at);
        match status {
            ProposalStatus::Passed { effective_at } => {
                assert_eq!(effective_at, closed_at.plus_days(RECALL_WINDOW_DAYS));
            }
            other => panic!("expected Passed, got {other:?}"),
        }
    }

    #[test]
    fn amendment_in_seed_phase_cannot_pass() {
        let mut p = amend_proposal();
        // Unanimous, full turnout — but Seed forbids constitutional change.
        let status = p.close(Tally { aye: 5, nay: 0 }, 5, Phase::Seed, Timestamp(3 * DAY));
        assert_eq!(status, ProposalStatus::Failed);
    }

    #[test]
    fn a_moderation_ballot_takes_effect_immediately_on_close() {
        let mut p = Proposal::new(
            ProposalId(2),
            ServerId(1),
            UserId(1),
            ProposalKind::RemoveContent { target: "msg-7".into() },
            Timestamp(0),
            Timestamp(DAY),
        );
        let closed_at = Timestamp(DAY);
        match p.close(Tally { aye: 6, nay: 5 }, 100, Phase::Sovereign, closed_at) {
            ProposalStatus::Passed { effective_at } => assert_eq!(effective_at, closed_at),
            other => panic!("expected immediate Passed, got {other:?}"),
        }
    }
}
