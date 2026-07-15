//! The minimum accepted password length.

/// Minimum password length. A high floor (matching the democratos sibling) is the
/// cheapest, most effective defense against online guessing — far more than
/// composition rules. Enforced by [`validate_password`](crate::validate_password).
pub const MIN_PASSWORD_LEN: usize = 16;
