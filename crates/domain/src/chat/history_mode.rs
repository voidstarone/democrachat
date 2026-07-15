//! How far back a channel's message history reaches for a new member.

use serde::{Deserialize, Serialize};

/// A channel's history policy — meaningful only for an **encrypted** channel,
/// where it decides how the channel key is distributed to members.
///
/// The server stores this as a flag; the *enforcement* is client-side, in how keys
/// are granted (the server is blind to the keys themselves):
/// - `Open` — one long-lived key for the channel's life. Every member, including a
///   later joiner, is granted that key, so a new member can read the whole backlog
///   (Discord-like).
/// - `Ephemeral` — the key ratchets on membership change: each change starts a new
///   key epoch, and a joiner is granted only the current epoch. They read from
///   their join onward, and a departed member cannot read anything sent after they
///   left (forward secrecy across membership changes, MLS-style).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMode {
    /// One long-lived key; new members read the full backlog.
    #[default]
    Open,
    /// Key ratchets on membership change; joiners see join-onward only.
    Ephemeral,
}
