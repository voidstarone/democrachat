//! Normalize a channel name the Discord way: lowercase, hyphenated, no spaces.

/// Normalize `input` into a Discord-style channel name: lowercase ASCII
/// alphanumerics kept, every run of other characters collapsed to a single
/// hyphen, leading/trailing hyphens trimmed. (This is the same shape as a server
/// slug — channels and servers share the convention.)
pub fn normalize_channel_name(input: &str) -> String {
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
    fn normalizes_like_discord() {
        assert_eq!(normalize_channel_name("General"), "general");
        assert_eq!(normalize_channel_name("off topic!!"), "off-topic");
        assert_eq!(normalize_channel_name("  # rules  "), "rules");
    }
}
