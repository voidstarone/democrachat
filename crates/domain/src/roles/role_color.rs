//! A validated `#rrggbb` colour a custom role can be tinted with.

use serde::{Deserialize, Serialize};

/// A role's display colour: a 6-digit `#rrggbb` hex string, normalized to
/// lowercase. Kept as a validated newtype because the client sets CSS `color:`
/// from it directly — an invalid or unbounded string must never reach the page.
/// Serializes transparently as the bare hex string, and deserializes back through
/// the same validation, so a hand-edited snapshot can't smuggle in a bad value.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RoleColor(String);

impl RoleColor {
    /// Parse a hex colour — `#rgb` or `#rrggbb`, with or without the leading `#` —
    /// and normalize to lowercase `#rrggbb`. `None` for anything else, so callers
    /// reject bad input rather than store it.
    pub fn parse(raw: &str) -> Option<RoleColor> {
        let hex = raw.trim();
        let hex = hex.strip_prefix('#').unwrap_or(hex).to_ascii_lowercase();
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let full = match hex.len() {
            // Shorthand: each nibble doubles (#abc → #aabbcc).
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex,
            _ => return None,
        };
        Some(RoleColor(format!("#{full}")))
    }

    /// The canonical `#rrggbb` string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RoleColor {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        RoleColor::parse(&s).ok_or_else(|| format!("invalid role colour: {s}"))
    }
}

impl From<RoleColor> for String {
    fn from(c: RoleColor) -> String {
        c.0
    }
}

#[cfg(test)]
mod tests {
    use super::RoleColor;

    #[test]
    fn six_digit_hex_is_normalized_lowercase() {
        assert_eq!(RoleColor::parse("#3B82F6").unwrap().as_str(), "#3b82f6");
        assert_eq!(RoleColor::parse("12ab34").unwrap().as_str(), "#12ab34");
    }

    #[test]
    fn shorthand_expands() {
        assert_eq!(RoleColor::parse("#fff").unwrap().as_str(), "#ffffff");
        assert_eq!(RoleColor::parse("0a5").unwrap().as_str(), "#00aa55");
    }

    #[test]
    fn junk_is_rejected() {
        assert_eq!(RoleColor::parse("blue"), None);
        assert_eq!(RoleColor::parse("#12"), None);
        assert_eq!(RoleColor::parse("#12xy56"), None);
        assert_eq!(RoleColor::parse(""), None);
        assert_eq!(RoleColor::parse("#"), None);
    }

    #[test]
    fn string_conversions_validate() {
        // The serde bridge (try_from/into "String") also validates on the way in.
        assert_eq!(RoleColor::try_from("#ABCDEF".to_string()).unwrap().as_str(), "#abcdef");
        assert!(RoleColor::try_from("nope".to_string()).is_err());
        assert_eq!(String::from(RoleColor::parse("#abcdef").unwrap()), "#abcdef");
    }
}
