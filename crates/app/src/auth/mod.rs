//! Authentication primitives: Argon2id password hashing/verification and a
//! timing equalizer for the account-miss path. Kept in `app` because auth is a
//! use-case concern; `domain` stays free of crypto.
pub mod hash_password;
pub mod spend_verify_time;
pub mod verify_password;
