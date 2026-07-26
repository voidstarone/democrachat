//! The result of an enfranchisement attempt.

use domain::Unmet;

/// What happened when a member asked to be enfranchised. Note every path here is
/// decided by the domain rules — there is no "admitted by an admin" arm, because
/// the franchise cannot be granted, only earned.
#[derive(Debug, PartialEq, Eq)]
pub enum EnfranchiseOutcome {
    /// The member met every criterion and a rate-cap slot was open: admitted as
    /// a citizen.
    Admitted,
    /// The member does not (yet) meet the franchise criteria. Carries the exact
    /// unmet requirements so the UI can say what's missing.
    NotEligible(Vec<Unmet>),
    /// The member qualifies, but the server's enfranchisement rate cap (Layer 2)
    /// is full for this window. They are not denied — only delayed. `admitted`
    /// and `cap` describe the window so the UI can explain the wait.
    RateCapped { admitted_this_window: u64 },
}

/// Where a member stands on a vote held **on trust** — seated as a founding member
/// before confirming their email address. Both fields are `false`/`None` for the
/// ordinary case of a member with nothing outstanding, so a UI can read it blindly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TrustedFranchise {
    /// Whole days left to confirm before the vote lapses, while it still stands.
    pub days_left: Option<i64>,
    /// The deadline passed with the address still unconfirmed: voting access is
    /// gone until they confirm.
    pub lapsed: bool,
}
