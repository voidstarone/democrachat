//! The maximum accepted password length.

/// Maximum password length. Argon2 hashes the full input, so an unbounded
/// password is a CPU-DoS vector (hash a megabyte per login attempt). This caps
/// it. Enforced by [`validate_password`](crate::validate_password).
pub const MAX_PASSWORD_LEN: usize = 256;
