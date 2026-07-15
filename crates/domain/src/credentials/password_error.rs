//! Why a proposed password was rejected.

use crate::{MAX_PASSWORD_LEN, MIN_PASSWORD_LEN};

/// Why a proposed password failed [`validate_password`](crate::validate_password).
///
/// Hand-written `Display`/`Error` (no `thiserror`) so `domain` stays dependency-
/// light — it depends only on `serde`.
#[derive(Debug, PartialEq, Eq)]
pub enum PasswordError {
    TooShort,
    TooLong,
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::TooShort => {
                write!(f, "password must be at least {MIN_PASSWORD_LEN} characters")
            }
            PasswordError::TooLong => {
                write!(f, "password must be at most {MAX_PASSWORD_LEN} characters")
            }
        }
    }
}

impl std::error::Error for PasswordError {}
