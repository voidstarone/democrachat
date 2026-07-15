use serde::{Deserialize, Serialize};

/// A forwardable write intent: a write a node wants applied to a scope it does not
/// own, sent to the owner to run and mint as a canonical change event.
///
/// Only the correctness-critical governance writes are federated as commands;
/// everything else replicates one-way through the feed. Extend as more cross-scope
/// writes need to travel to their owner.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Command {
    /// Cast a governance ballot. The owner re-runs its own vote use-case (checking
    /// the voter's citizenship, the proposal's phase, etc.) and mints the canonical
    /// `votes` change event — the forwarder is trusted only to *relay* the intent,
    /// never to decide it.
    CastVote { proposal: u64, voter: u64, aye: bool },
    /// Send a sealed direct message. Routed to the **sender's** home (which owns the
    /// sender's DMs); the owner re-checks the `can_dm` gate and mints the canonical
    /// `dms` change event. The two ciphertexts are opaque — the body is E2EE.
    SendDm {
        from: u64,
        to: u64,
        sealed_for_recipient: String,
        sealed_for_sender: String,
    },
    /// Permanently block a user. Safety-critical: a block must silence DMs in **both**
    /// directions, and the DM gate runs on the *sender's* home — so the block has to
    /// land on **both** users' homes before it can be trusted. Unlike every other
    /// command it therefore targets **two** scopes (`UserHome(blocker)` and
    /// `UserHome(blocked)`) and is committed to both synchronously, not left to the
    /// eventual feed. Idempotent, so a partial commit is safe to re-drive.
    Block { blocker: u64, blocked: u64 },
    /// Send (or re-affirm) a friend request. A friendship is a two-user record: the
    /// addressee's home shows the incoming request, the requester's the outgoing one.
    /// Targets both homes (`UserHome(requester)` + `UserHome(addressee)`), committed
    /// synchronously. Idempotent.
    RequestFriend { requester: u64, addressee: u64 },
    /// Accept a pending friend request. Once accepted the friends-only DM gate reads
    /// the friendship on the *sender's* home, so the accepted state must land on both
    /// homes (`UserHome(accepter)` + `UserHome(requester)`) synchronously.
    AcceptFriend { accepter: u64, requester: u64 },
}
