//! The scope(s) a command writes into — resolved the same way on the caller (to
//! route it) and on the owner (to authorize it), so the two never disagree.

use federation::{OwnedScope, ScopeResolver};

use crate::command::command::Command;

/// The scope(s) a command's write belongs to. Whoever owns a listed scope is a node
/// that must run the command; a command with more than one scope (a block) must be
/// committed on **every** listed scope's owner. Empty when no scope can be resolved
/// yet (e.g. the parent proposal isn't replicated), which callers treat as "no
/// reachable owner".
pub async fn target_scopes(cmd: &Command, resolver: &dyn ScopeResolver) -> Vec<OwnedScope> {
    match cmd {
        Command::CastVote { proposal, .. } => resolver
            .proposal_server(*proposal)
            .await
            .map(OwnedScope::Server)
            .into_iter()
            .collect(),
        // A DM is owned by its sender's home — no parent lookup needed.
        Command::SendDm { from, .. } => vec![OwnedScope::UserHome(*from)],
        // A block is owned jointly by both users' homes: whichever of them a later
        // DM is sent from must already hold the block, so both must commit it.
        Command::Block { blocker, blocked } => {
            vec![OwnedScope::UserHome(*blocker), OwnedScope::UserHome(*blocked)]
        }
        // A friendship is likewise a two-user record — both homes hold it (for
        // request visibility and, once accepted, the friends-only DM gate).
        Command::RequestFriend { requester, addressee } => {
            vec![OwnedScope::UserHome(*requester), OwnedScope::UserHome(*addressee)]
        }
        Command::AcceptFriend { accepter, requester } => {
            vec![OwnedScope::UserHome(*accepter), OwnedScope::UserHome(*requester)]
        }
    }
}
