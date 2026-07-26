//! Whether a confirmed email address is a condition of the franchise.

/// Whether a member must have confirmed their email address before they can hold
/// the franchise.
///
/// This is *deployment* policy — an operator's choice about how much identity a
/// vote costs — and deliberately **not** part of [`FranchiseCriteria`](crate::FranchiseCriteria),
/// which is the per-server bar that members themselves vote on. A server cannot
/// vote its way out of the operator's email requirement, and the operator cannot
/// use it to set a server's age or contribution bar. It arrives as an argument to
/// [`evaluate_eligibility`](crate::evaluate_eligibility) so the two stay separate
/// while still meeting at the one function allowed to conclude "eligible".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmailFranchiseRule {
    /// Email confirmation has no bearing on the franchise (verification off).
    #[default]
    Ignored,
    /// An unconfirmed address keeps the member off the roll until they confirm.
    MustBeConfirmed,
}
