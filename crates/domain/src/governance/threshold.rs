//! A passing bar in basis points.

use serde::{Deserialize, Serialize};

/// A passing bar, expressed in basis points (1/10_000) to keep the rules in
/// exact integer arithmetic — no floating-point drift in governance outcomes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Threshold {
    /// Fraction of *cast* votes that must be "aye" (strictly exceeded).
    pub approval_bp: u32,
    /// Fraction of *established citizens* that must cast a vote (turnout).
    pub quorum_bp: u32,
}
