//! Canonicalize a role name into its mention token.

/// Normalize a role name to the token used in an `@mention`: trimmed, lowercased,
/// inner whitespace collapsed to single hyphens, and restricted to
/// `[a-z0-9_-]`. Mirrors [`normalize_channel_name`](crate::normalize_channel_name)
/// so a role reads like a handle. An all-invalid input normalizes to empty, which
/// callers reject.
pub fn normalize_role_name(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if (ch.is_whitespace() || ch == '-') && !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
