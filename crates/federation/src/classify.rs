//! Classify a change event by the scope it authoritatively belongs to.

use crate::derived_scope::DerivedScope;
use crate::ownership::owned_scope::OwnedScope;
use crate::signed_part::SignedPart;

/// Read a `u64` id from a payload field, tolerating a string encoding.
fn payload_u64(payload: &serde_json::Value, key: &str) -> Option<u64> {
    match payload.get(key)? {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Classify an event by the scope it authoritatively belongs to, reading **only**
/// its payload — the entity→scope map mirrors the store's replication set. An
/// entity outside it is [`DerivedScope::Indeterminate`] (refused).
///
/// democrachat's rows mostly carry their scope id directly (`server_id`, or a user
/// id for the social graph), so most entities resolve without a parent lookup;
/// only votes and reactions need one (see [`crate::ScopeResolver`]).
pub fn classify(part: &SignedPart) -> DerivedScope {
    let p = &part.payload;
    let server = |key: &str| match payload_u64(p, key) {
        Some(s) => DerivedScope::Owned(OwnedScope::Server(s)),
        None => DerivedScope::Indeterminate,
    };
    let home = |key: &str| match payload_u64(p, key) {
        Some(u) => DerivedScope::Owned(OwnedScope::UserHome(u)),
        None => DerivedScope::Indeterminate,
    };
    match part.entity.as_str() {
        // The server row itself is keyed by its own id.
        "servers" => server("id"),
        // Server-scoped rows that carry their server directly.
        "channels" | "messages" | "proposals" | "memberships" | "rules" | "roles"
        | "role_assignments" | "emojis" | "emoji_votes" | "channel_grants" | "invites" => {
            server("server_id")
        }
        // Parent-scoped: a vote's server is its proposal's; a reaction's is its message's.
        "votes" => match payload_u64(p, "proposal_id") {
            Some(id) => DerivedScope::ViaProposal(id),
            None => DerivedScope::Indeterminate,
        },
        "reactions" => match payload_u64(p, "message_id") {
            Some(id) => DerivedScope::ViaMessage(id),
            None => DerivedScope::Indeterminate,
        },
        // User-global social graph — homed by a user id. A DM/friendship/block is
        // authored by the initiating user's home node, which owns the write.
        "users" => home("id"),
        // A user's published device keys are homed on that user's node.
        "user_keys" => home("user_id"),
        "dms" => home("sender"),
        "friendships" => home("requester"),
        "blocks" => home("blocker"),
        _ => DerivedScope::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_op::ChangeOp;
    use crate::event_scope::EventScope;

    fn part(entity: &str, payload: serde_json::Value) -> SignedPart {
        SignedPart {
            node: 1,
            epoch: 1,
            seq: 1,
            scope: EventScope::Global, // envelope scope is ignored by classify
            entity: entity.into(),
            op: ChangeOp::Upsert,
            payload,
        }
    }

    #[test]
    fn scope_is_derived_from_the_payload_not_the_envelope() {
        // A message carries its server directly — even though the envelope lies (Global).
        assert_eq!(
            classify(&part("messages", serde_json::json!({ "id": 5, "server_id": 8 }))),
            DerivedScope::Owned(OwnedScope::Server(8))
        );
        // A DM homes by its sender.
        assert_eq!(
            classify(&part("dms", serde_json::json!({ "id": 1, "sender": 42, "recipient": 9 }))),
            DerivedScope::Owned(OwnedScope::UserHome(42))
        );
        // A block homes by its blocker (safety row).
        assert_eq!(
            classify(&part("blocks", serde_json::json!({ "blocker": 7, "blocked": 3 }))),
            DerivedScope::Owned(OwnedScope::UserHome(7))
        );
        // A user's published device keys home on that user's node.
        assert_eq!(
            classify(&part("user_keys", serde_json::json!({ "user_id": 42, "public_key": "ab" }))),
            DerivedScope::Owned(OwnedScope::UserHome(42))
        );
        // A vote defers to its proposal.
        assert_eq!(
            classify(&part("votes", serde_json::json!({ "proposal_id": 100, "voter": 1 }))),
            DerivedScope::ViaProposal(100)
        );
        // Custom emoji and their votes carry their server directly.
        assert_eq!(
            classify(&part("emojis", serde_json::json!({ "id": 3, "server_id": 8 }))),
            DerivedScope::Owned(OwnedScope::Server(8))
        );
        assert_eq!(
            classify(&part("emoji_votes", serde_json::json!({ "server_id": 8, "emoji_id": 3, "voter": 1 }))),
            DerivedScope::Owned(OwnedScope::Server(8))
        );
        // An encrypted channel's key grants carry their server directly.
        assert_eq!(
            classify(&part(
                "channel_grants",
                serde_json::json!({ "server_id": 8, "channel_id": 2, "epoch": 0, "member": 1 }),
            )),
            DerivedScope::Owned(OwnedScope::Server(8))
        );
        // A reaction defers to its message.
        assert_eq!(
            classify(&part("reactions", serde_json::json!({ "message_id": 55, "user": 1 }))),
            DerivedScope::ViaMessage(55)
        );
        // An unknown entity, or a missing scope id, is refused.
        assert_eq!(classify(&part("gremlins", serde_json::json!({}))), DerivedScope::Indeterminate);
        assert_eq!(
            classify(&part("messages", serde_json::json!({ "id": 5 }))),
            DerivedScope::Indeterminate
        );
    }
}
