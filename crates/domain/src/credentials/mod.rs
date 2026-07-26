//! Credential policy — pure rules about passwords. Hashing/verification is an
//! `app`-layer concern (it needs a CSPRNG and Argon2); only the length policy,
//! which is pure, lives here.
pub mod email_error;
pub mod max_email_len;
pub mod max_password_len;
pub mod min_password_len;
pub mod password_error;
pub mod validate_email;
pub mod validate_password;
