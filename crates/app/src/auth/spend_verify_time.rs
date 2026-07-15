//! Burn a verify's worth of CPU on a dummy hash, to flatten login timing.

use std::sync::OnceLock;

use crate::auth::hash_password::hash_password;
use crate::auth::verify_password::verify_password;

/// A cached Argon2 hash of a throwaway password, computed once.
static DUMMY_HASH: OnceLock<String> = OnceLock::new();

/// Perform one Argon2 verification against a dummy hash and discard the result.
///
/// Called on the login paths where there is **no** real hash to check — account
/// not found, or a passwordless account — so those paths cost the same as a real
/// verify. Without this, an attacker times the response to enumerate which
/// handles exist. The dummy hash is computed once and reused.
pub fn spend_verify_time() {
    let dummy = DUMMY_HASH.get_or_init(|| {
        hash_password("timing-equalizer-placeholder-value")
            .unwrap_or_else(|_| String::from("$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"))
    });
    // The password will not match; we only want the CPU cost.
    let _ = verify_password("wrong-password-on-purpose", dummy);
}
