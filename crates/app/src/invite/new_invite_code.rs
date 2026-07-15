//! Mint a fresh, unguessable invite code.

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

/// A new invite code: 16 alphanumeric characters (~95 bits) drawn from the OS-seeded
/// thread CSPRNG. This is the *raw* secret the minter shares out of band; it is never
/// stored — only its [`hash_code`](crate::invite::hash_code::hash_code) digest is
/// persisted, so a leaked snapshot yields no working invites.
pub fn new_invite_code() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_16_alphanumeric_and_unpredictable() {
        let a = new_invite_code();
        let b = new_invite_code();
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b, "two draws must not collide");
    }
}
