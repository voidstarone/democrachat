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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TrialId,
        server_id: ServerId,
        report_id: ReportId,
        accused: UserId,
        jurors: Vec<UserId>,
        jury_weight: u64,
        juror_weights: Vec<u64>,
        opened_at: Timestamp,
        closes_at: Timestamp,
    ) -> Self {
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
