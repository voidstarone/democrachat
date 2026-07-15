//! A jury verdict.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Verdict {
    /// Not all decisive votes are in yet.
    Pending,
    Guilty,
    NotGuilty,
}
