//! Hash a password with Argon2id.

use argon2::Argon2;
use password_hash::{PasswordHasher, SaltString};
use rand::rngs::OsRng;

/// Hash `password` with Argon2 (default params: Argon2id) and a fresh random
/// per-password salt, returning a self-describing PHC string safe to store. The
/// salt is embedded in the output, so no salt column is needed.
pub fn hash_password(password: &str) -> Result<String, password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default().hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}
