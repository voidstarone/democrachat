//! What a change event is scoped to — the axis it replicates along.
//!
//! democrachat has two sharding axes (see `docs/federation.md` §2), so unlike
//! democratos's single community scope, an event names which axis it belongs to.
//! Binding the scope into the signature stops an event being replayed against a
//! different server or user, and tells a consumer which owner's key to verify
//! against and which replica to apply it to.

use serde::{Deserialize, Serialize};

/// The entity a change is scoped to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum EventScope {
    /// Server-scoped governance/content (channels, messages, proposals, votes,
    /// roles, rules, emoji, memberships) — owned by the server's current owner.
    Server(u64),
    /// The user-global social graph (accounts, DMs, friends, blocks) — owned by
    /// the user's home node.
    UserHome(u64),
    /// A change bound to no single server or user (e.g. the global directory).
    Global,
}
