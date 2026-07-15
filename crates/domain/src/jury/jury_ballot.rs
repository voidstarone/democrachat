//! A single juror's ballot.

use serde::{Deserialize, Serialize};

use crate::{TrialId, UserId};

/// A single juror's ballot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct JuryBallot {
    pub trial_id: TrialId,
    pub juror: UserId,
    pub guilty: bool,
}
