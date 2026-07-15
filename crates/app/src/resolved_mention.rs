//! One resolved `@mention` in a server message.
//!
//! [`domain::parse_mentions`] extracts the raw tokens; resolving each token to a
//! user, a role, or nothing needs the stores, so it lives here in `app`. The web
//! layer uses the result both to highlight a message and to know who was pinged.

use crate::MentionKind;

/// One resolved mention: the token as written, what it is, and the handles it
/// addresses (a single handle for a user; the group for a role; empty for an
/// unknown token).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedMention {
    /// The bare token, without the leading `@` (lowercased).
    pub token: String,
    pub kind: MentionKind,
    /// The member handles this mention addresses.
    pub handles: Vec<String>,
}

impl ResolvedMention {
    /// Whether this mention resolved to something real (a user or a role).
    pub fn is_resolved(&self) -> bool {
        !matches!(self.kind, MentionKind::Unknown)
    }
}
