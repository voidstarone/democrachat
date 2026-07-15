//! The result of checking a member against the franchise criteria.

use serde::{Deserialize, Serialize};

use crate::Unmet;

/// The result of checking a member against the franchise criteria.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Eligibility {
    pub unmet: Vec<Unmet>,
}

impl Eligibility {
    /// Eligible exactly when nothing is unmet. Note: eligibility is necessary
    /// but not sufficient — admission is still gated by the rate cap (Layer 2).
    pub fn is_eligible(&self) -> bool {
        self.unmet.is_empty()
    }
}
