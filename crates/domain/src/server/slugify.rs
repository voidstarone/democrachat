//! Turn a free-text handle into a URL-safe server slug.

/// Normalize `input` into a lowercase, hyphen-separated slug: ASCII
/// alphanumerics are kept, every run of other characters becomes a single
/// hyphen, and leading/trailing hyphens are trimmed. Deterministic and pure.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_hyphen = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes() {
        assert_eq!(slugify("Rustaceans"), "rustaceans");
        assert_eq!(slugify("  Hello, World!  "), "hello-world");
        assert_eq!(slugify("multi   space"), "multi-space");
        assert_eq!(slugify("--edge--"), "edge");
    }
}
