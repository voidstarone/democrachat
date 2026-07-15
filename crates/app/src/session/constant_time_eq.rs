//! Constant-time byte comparison.

use subtle::ConstantTimeEq;

/// Compare two byte slices in constant time (w.r.t. their contents), so a caller
/// can't learn *where* a mismatch is by timing. Used for session MACs, CSRF
/// tokens, and any secret comparison. Slices of different lengths are unequal.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}
