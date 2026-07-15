//! The resolved meaning of an `@mention` in a server message.
//!
//! [`domain::parse_mentions`] extracts the raw tokens; resolving each token to a
//! user, a role, or nothing needs the stores, so it lives here in `app`. The web
//! layer uses the result both to highlight a message and to know who was pinged.

/// What an `@token` turned out to refer to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MentionKind {
    /// A member of the server, addressed by handle.
    User,
    /// A built-in [`StandingRole`](domain::StandingRole): `@everyone` etc.
    StandingRole,
    /// A custom, ballot-created role.
    Role,
    /// Matched no user or role on this server.
    Unknown,
}

impl MentionKind {
    /// A stable lowercase tag for wire/UI use.
    pub fn as_str(&self) -> &'static str {
        match self {
            MentionKind::User => "user",
            MentionKind::StandingRole => "standing_role",
            MentionKind::Role => "role",
            MentionKind::Unknown => "unknown",
        }
    }
}

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
