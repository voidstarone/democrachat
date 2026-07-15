//! A set of tags kept as a single pipe-fenced string.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A set of tags on a server, channel, or user, kept as ONE pipe-fenced string —
/// `|rust|gaming|` — so an exact-tag test is a plain substring match for `|rust|`.
/// That is fast and index-friendly (Postgres `LIKE '%|rust|%'`, no join table) and
/// the fences stop `|rust|` matching `|rustlang|`. The empty set is the empty
/// string.
///
/// Tags are normalized (trimmed, lowercased, pipes stripped) and deduplicated on
/// the way in, and kept sorted, so two equal sets have byte-equal strings — the
/// type can derive `PartialEq`/`Eq` and round-trip through serde transparently.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tags(String);

impl Tags {
    /// Build from individual tag strings, normalizing, deduplicating, and sorting.
    pub fn from_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let set: BTreeSet<String> = tags.into_iter().filter_map(|t| normalize(t.as_ref())).collect();
        Self::from_set(&set)
    }

    /// Parse a user's free-form input — tags separated by commas or whitespace.
    pub fn from_input(raw: &str) -> Self {
        Self::from_tags(raw.split(|c: char| c == ',' || c.is_whitespace()))
    }

    fn from_set(set: &BTreeSet<String>) -> Self {
        if set.is_empty() {
            return Self(String::new());
        }
        let mut s = String::with_capacity(set.iter().map(|t| t.len() + 1).sum::<usize>() + 1);
        s.push('|');
        for t in set {
            s.push_str(t);
            s.push('|');
        }
        Self(s)
    }

    /// Whether `tag` is present (matched normalized, case-insensitively).
    pub fn contains(&self, tag: &str) -> bool {
        match normalize(tag) {
            Some(t) => self.0.contains(&format!("|{t}|")),
            None => false,
        }
    }

    /// The tags, in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.split('|').filter(|s| !s.is_empty())
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The raw pipe-fenced storage form (`""` when empty, else `|a|b|`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The substring a store searches for to find this exact tag — the normalized
    /// tag wrapped in its fences, e.g. `rust` → `|rust|`. `None` if the term is
    /// blank once normalized (nothing to match). The pipe fencing is entirely
    /// encapsulated here: callers pass a plain tag and never see a `|`.
    pub fn search_needle(tag: &str) -> Option<String> {
        let one = Self::from_tags([tag]);
        if one.is_empty() {
            None
        } else {
            Some(one.0)
        }
    }
}

/// Normalize one tag: trim, lowercase, drop the pipe fence character; `None` if
/// nothing is left (so blank entries in free-form input just fall away).
fn normalize(raw: &str) -> Option<String> {
    let t: String = raw.trim().to_lowercase().chars().filter(|c| *c != '|').collect();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_is_the_empty_string() {
        let t = Tags::default();
        assert!(t.is_empty());
        assert_eq!(t.as_str(), "");
        assert!(!t.contains("anything"));
    }

    #[test]
    fn tags_are_fenced_sorted_and_deduped() {
        let t = Tags::from_tags(["Gaming", "rust", "gaming"]);
        assert_eq!(t.as_str(), "|gaming|rust|");
        assert_eq!(t.iter().collect::<Vec<_>>(), vec!["gaming", "rust"]);
    }

    #[test]
    fn contains_is_exact_not_prefix() {
        let t = Tags::from_tags(["rust"]);
        assert!(t.contains("rust"));
        assert!(t.contains("RUST"), "match is case-insensitive");
        assert!(!t.contains("rus"));
        assert!(!t.contains("rustlang"), "the fences stop a prefix match");
    }

    #[test]
    fn free_form_input_splits_on_commas_and_whitespace() {
        let t = Tags::from_input("  rust, gaming\tweb  ");
        assert_eq!(t.as_str(), "|gaming|rust|web|");
    }

    #[test]
    fn pipes_in_a_tag_are_stripped_so_the_fence_cannot_be_forged() {
        let t = Tags::from_tags(["a|b"]);
        assert_eq!(t.as_str(), "|ab|");
        assert!(t.contains("ab"));
    }

    #[test]
    fn a_search_needle_is_the_fenced_normalized_tag() {
        assert_eq!(Tags::search_needle("Rust").as_deref(), Some("|rust|"));
        assert_eq!(Tags::search_needle("  gaming "), Some("|gaming|".to_string()));
        assert_eq!(Tags::search_needle("   "), None);
        assert_eq!(Tags::search_needle("|"), None);
    }

    #[test]
    fn serde_round_trips_transparently_as_the_string() {
        let t = Tags::from_tags(["rust", "gaming"]);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"|gaming|rust|\"");
        let back: Tags = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
