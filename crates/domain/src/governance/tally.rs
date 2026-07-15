//! The aye/nay counts for a proposal.

use serde::{Deserialize, Serialize};

/// The aye/nay counts for a proposal, in units of vote weight (which equal head
/// counts under one-citizen-one-vote).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Tally {
    pub aye: u64,
    pub nay: u64,
}

impl Tally {
    pub fn cast(&self) -> u64 {
        self.aye + self.nay
    }
}
