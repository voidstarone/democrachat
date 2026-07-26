//! Email verification: the on-signup flow that proves a user controls an address.
//!
//! Emails are stored **only encrypted** (see [`field_cipher`]) under a dedicated
//! key; a plaintext address is never persisted. Ownership is proven by mailing a
//! high-entropy token whose hash — not the token — is stored ([`token`]).

pub mod field_cipher;
pub mod token;
pub mod verification_mode;

pub use field_cipher::{open_email, seal_email};
pub use token::{hash_token, new_verification_token};
pub use verification_mode::EmailVerificationMode;

/// Canonical form used only for **uniqueness comparison** (never for delivery):
/// trimmed and ASCII-lowercased, so `Alice@Example.com` and `alice@example.com`
/// are treated as the same address.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}
