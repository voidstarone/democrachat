//! A jury trial.

use serde::{Deserialize, Serialize};

use crate::{ServerId, ReportId, Timestamp, TrialId, UserId, Verdict};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Trial {
    pub id: TrialId,
    pub server_id: ServerId,
    pub report_id: ReportId,
    pub accused: UserId,
    pub jurors: Vec<UserId>,
    /// The total vote weight of the empanelled jury, frozen at selection. The
    /// conviction bar (2/3 supermajority) is measured against this, so it stays
    /// fixed even if a juror's weight later changes. Under one-juror-one-vote it
    /// equals `jurors.len()`.
    #[serde(default)]
    pub jury_weight: u64,
    /// Each juror's vote weight, frozen at empanelment and aligned by index with
    /// `jurors`. The verdict tally weighs each ballot by the juror's *frozen*
    /// weight here — not a live recomputation — so a juror cannot shift the 2/3
    /// conviction bar mid-trial. Empty for one-juror-one-vote juries, where
    /// [`Trial::juror_weight`] returns 1.
    #[serde(default)]
    pub juror_weights: Vec<u64>,
    pub opened_at: Timestamp,
    pub closes_at: Timestamp,
    pub verdict: Verdict,
}

impl Trial {
    /// Empanel a jury. `jury` pairs each juror with the vote weight frozen at
    /// selection; passing the `(juror, weight)` pairs as one list is what keeps
    /// `jurors`, `juror_weights`, and `jury_weight` in lockstep — a caller can no
    /// longer desync the parallel arrays and corrupt the conviction bar. The bar
    /// (`jury_weight`) is computed here as the true sum of the frozen weights.
    ///
    /// A one-juror-one-vote jury (every weight 1) is stored with **no** per-juror
    /// weights, so [`Trial::juror_weight`] returns 1 and `jury_weight` is the head
    /// count — the unweighted case a weighted jury degenerates to.
    pub fn new(
        id: TrialId,
        server_id: ServerId,
        report_id: ReportId,
        accused: UserId,
        jury: Vec<(UserId, u64)>,
        opened_at: Timestamp,
        closes_at: Timestamp,
    ) -> Self {
        let jury_weight = jury.iter().map(|&(_, w)| w).sum();
        let is_unweighted = jury.iter().all(|&(_, w)| w == 1);
        let jurors = jury.iter().map(|&(u, _)| u).collect();
        let juror_weights = if is_unweighted {
            Vec::new()
        } else {
            jury.into_iter().map(|(_, w)| w).collect()
        };
        Self {
            id,
            server_id,
            report_id,
            accused,
            jurors,
            jury_weight,
            juror_weights,
            opened_at,
            closes_at,
            verdict: Verdict::Pending,
        }
    }

    pub fn is_juror(&self, user: UserId) -> bool {
        self.jurors.contains(&user)
    }

    /// This juror's vote weight, frozen at empanelment. Returns 1 when the trial
    /// carries no frozen weights (an unweighted jury) or the user was not on the
    /// panel.
    pub fn juror_weight(&self, user: UserId) -> u64 {
        self.jurors
            .iter()
            .position(|j| *j == user)
            .and_then(|i| self.juror_weights.get(i).copied())
            .unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trial(jury: Vec<(UserId, u64)>) -> Trial {
        Trial::new(TrialId(1), ServerId(1), ReportId(1), UserId(9), jury, Timestamp(0), Timestamp(10))
    }

    /// An unweighted jury freezes no per-juror weights: `jury_weight` is the head
    /// count and every juror's weight is 1.
    #[test]
    fn an_unweighted_jury_is_one_juror_one_vote() {
        let t = trial(vec![(UserId(1), 1), (UserId(2), 1), (UserId(3), 1)]);
        assert_eq!(t.jury_weight, 3, "the bar is the head count");
        assert!(t.juror_weights.is_empty(), "no per-juror weights are stored");
        assert_eq!(t.juror_weight(UserId(2)), 1);
        assert!(t.is_juror(UserId(2)));
        assert!(!t.is_juror(UserId(4)));
    }

    /// A weighted jury freezes each ballot's weight index-aligned to `jurors`, and
    /// `jury_weight` is their true sum — the invariant the old parallel-array
    /// constructor let a caller violate.
    #[test]
    fn a_weighted_jury_freezes_aligned_weights_summing_to_the_bar() {
        let t = trial(vec![(UserId(1), 2), (UserId(2), 3), (UserId(3), 1)]);
        assert_eq!(t.jury_weight, 6, "jury_weight is the sum of frozen weights");
        assert_eq!(t.jurors, vec![UserId(1), UserId(2), UserId(3)]);
        assert_eq!(t.juror_weights, vec![2, 3, 1], "weights stay aligned to jurors");
        assert_eq!(t.juror_weight(UserId(1)), 2);
        assert_eq!(t.juror_weight(UserId(2)), 3);
        assert_eq!(t.juror_weight(UserId(3)), 1);
        assert_eq!(t.juror_weight(UserId(99)), 1, "a non-juror defaults to 1");
    }

    #[test]
    fn a_fresh_trial_opens_pending() {
        assert_eq!(trial(vec![(UserId(1), 1)]).verdict, Verdict::Pending);
    }
}
