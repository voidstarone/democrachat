//! What an `@token` in a server message turned out to refer to.

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
