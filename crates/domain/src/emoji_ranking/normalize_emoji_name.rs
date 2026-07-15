//! Normalize a custom emoji's short name.

/// Normalize an emoji `:name:` to its canonical form: lowercase, and only
/// `a–z 0–9 - _` (so it is safe to type between colons and unique per server). An
/// input that normalizes to empty is rejected by the caller.
pub fn normalize_emoji_name(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}
