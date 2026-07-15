//! A scope that a node can *own* — the claimable subset of [`EventScope`].
//!
//! democrachat has two ownable axes (see `docs/federation.md` §2), and a
//! `ServerId` and a `UserId` can be numerically equal (both are `compose_id`
//! values). So ownership must be keyed by the **typed** scope, not a bare `u64` —
//! otherwise a server and a user-home sharing a number would be conflated. This
//! is the one place democrachat's control plane diverges from democratos's
//! single-community scope.

use serde::{Deserialize, Serialize};

use crate::event_scope::EventScope;

/// Something a node can hold ownership of: a server, or a user's home. The
/// `Global` events of [`EventScope`] have no per-scope owner (their authority is
/// the minting node), so they are deliberately not representable here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum OwnedScope {
    /// A self-governing server and all its governance/content.
    Server(u64),
    /// A user's home — their account and the social graph homed on it.
    UserHome(u64),
}

impl OwnedScope {
    /// The ownable scope an event belongs to, or `None` for a `Global` event
    /// (which has no per-scope owner to consult).
    pub fn from_event(scope: EventScope) -> Option<Self> {
        match scope {
            EventScope::Server(id) => Some(OwnedScope::Server(id)),
            EventScope::UserHome(id) => Some(OwnedScope::UserHome(id)),
            EventScope::Global => None,
        }
    }
}

impl From<OwnedScope> for EventScope {
    fn from(s: OwnedScope) -> Self {
        match s {
            OwnedScope::Server(id) => EventScope::Server(id),
            OwnedScope::UserHome(id) => EventScope::UserHome(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_events_have_no_ownable_scope() {
        assert_eq!(OwnedScope::from_event(EventScope::Global), None);
        assert_eq!(
            OwnedScope::from_event(EventScope::Server(7)),
            Some(OwnedScope::Server(7))
        );
        assert_eq!(
            OwnedScope::from_event(EventScope::UserHome(7)),
            Some(OwnedScope::UserHome(7))
        );
    }

    #[test]
    fn a_server_and_a_user_home_with_the_same_number_are_distinct() {
        // The whole reason ownership is keyed by the typed scope, not a bare u64.
        assert_ne!(OwnedScope::Server(42), OwnedScope::UserHome(42));
    }
}
