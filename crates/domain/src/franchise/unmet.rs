//! A single unmet franchise requirement.

use serde::{Deserialize, Serialize};

/// A single unmet requirement, carrying both the bar and the member's standing
/// so the UI can say "you need 30 days, you have 12" without re-deriving it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Unmet {
    AccountTooYoung { need_days: i64, have_days: i64 },
    MembershipTooShort { need_days: i64, have_days: i64 },
    InsufficientContribution { need: i64, have: i64 },
    Sanctioned,
    /// The account is permanently barred from the franchise (a dev/content
    /// puppet). No criterion can lift this — see
    /// [`crate::User::is_franchise_barred`].
    Barred,
}
