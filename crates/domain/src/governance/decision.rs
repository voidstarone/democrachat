//! The outcome of applying a threshold to a tally.

use serde::{Deserialize, Serialize};

/// The outcome of applying a threshold to a tally, with the reasons broken out
/// so the UI can explain *why* something failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub did_pass: bool,
    pub has_quorum: bool,
    pub has_approval: bool,
}
