//! Extract `@mention` tokens from a message body.

/// Scan `body` for `@mention` tokens and return the distinct bare tokens (without
/// the leading `@`), in first-appearance order.
///
/// A token is `@` followed by one or more of `[A-Za-z0-9_-]`, matching the handle
/// and role-name character set. The `@` must start the string or follow a
/// non-word character, so an email address (`a@b`) does not read as a mention.
/// Resolution — is a token a user, a role, or nothing? — is an `app`-layer concern
/// that needs the stores; this function is pure text.
///
/// The returned tokens are lowercased so they can be matched against normalized
/// role names and (case-insensitively) against handles.
pub fn parse_mentions(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            // The `@` must open the string or follow a non-word character, so we
            // don't treat the `@` inside `foo@bar` as a mention.
            let preceded_ok = i == 0 || !is_word_byte(bytes[i - 1]);
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && is_word_byte(bytes[j]) {
                j += 1;
            }
            if preceded_ok && j > start {
                let token = body[start..j].to_lowercase();
                if !out.contains(&token) {
                    out.push(token);
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Whether `b` is part of a mention token (or a preceding "word" that blocks one).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_simple_user_mention() {
        assert_eq!(parse_mentions("hey @ada look"), vec!["ada"]);
    }

    #[test]
    fn extracts_multiple_and_dedupes_preserving_order() {
        assert_eq!(
            parse_mentions("@bob @carol @bob @citizens"),
            vec!["bob", "carol", "citizens"]
        );
    }

    #[test]
    fn lowercases_tokens() {
        assert_eq!(parse_mentions("@Ada and @BOB"), vec!["ada", "bob"]);
    }

    #[test]
    fn ignores_at_inside_a_word_like_an_email() {
        assert_eq!(parse_mentions("mail me at a@b.com"), Vec::<String>::new());
    }

    #[test]
    fn a_lone_at_is_not_a_mention() {
        assert_eq!(parse_mentions("a @ b"), Vec::<String>::new());
    }

    #[test]
    fn handles_punctuation_after_the_token() {
        assert_eq!(parse_mentions("thanks @ada!"), vec!["ada"]);
    }

    #[test]
    fn accepts_hyphens_and_underscores() {
        assert_eq!(parse_mentions("@night-owls @power_users"), vec!["night-owls", "power_users"]);
    }
}
