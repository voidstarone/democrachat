//! Deterministic jury selection.

use crate::UserId;

/// Deterministically select up to `size` jurors from `candidates`, excluding the
/// accused. Selection is a stable hash ordering keyed by `seed`, so the same
/// inputs always yield the same jury — auditable and reproducible.
pub fn select_jury(candidates: &[UserId], accused: UserId, size: usize, seed: u64) -> Vec<UserId> {
    let mut scored: Vec<(u64, UserId)> = candidates
        .iter()
        .copied()
        .filter(|&c| c != accused)
        .map(|c| (mix(seed ^ c.0), c))
        .collect();
    // Sort by hash, tie-break by id for total determinism.
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1 .0.cmp(&b.1 .0)));
    scored.truncate(size);
    scored.into_iter().map(|(_, id)| id).collect()
}

/// SplitMix64 finalizer — a fast, well-distributed hash for seeded selection.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users(n: u64) -> Vec<UserId> {
        (1..=n).map(UserId).collect()
    }

    #[test]
    fn jury_selection_is_deterministic_and_excludes_accused() {
        let members = users(100);
        let accused = UserId(42);
        let j1 = select_jury(&members, accused, 7, 12345);
        let j2 = select_jury(&members, accused, 7, 12345);
        assert_eq!(j1, j2, "same seed -> same jury");
        assert_eq!(j1.len(), 7);
        assert!(!j1.contains(&accused));

        let j3 = select_jury(&members, accused, 7, 999);
        assert_ne!(j1, j3);
    }

    #[test]
    fn jury_caps_at_available_members() {
        let members = users(4);
        let jury = select_jury(&members, UserId(1), 7, 1);
        assert_eq!(jury.len(), 3); // 4 members minus the accused
    }
}
