//! A channel key handed to one member — the channel key sealed to their device key.

use serde::{Deserialize, Serialize};

use crate::{ChannelId, ServerId, UserId};

/// One member's copy of an encrypted channel's key, for a given key epoch. The
/// symmetric channel key is sealed to the member's **device public key** (from the
/// key directory), so only that member can open it. The server stores the sealed
/// blob verbatim and can never recover the key.
///
/// Grants are produced **client-side** by a member who already holds the key
/// (there is no server-side key escrow): when a channel is created or a member is
/// admitted, an existing member seals the key to the newcomer and uploads this. The
/// `server_id` is carried so the federation can shard/authorize the row by server
/// without a channel lookup.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ChannelKeyGrant {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
    /// Which key epoch this grants — `Open` channels use only epoch 0; `Ephemeral`
    /// channels advance it on each membership change.
    pub epoch: u32,
    /// The member who can open `sealed_key`.
    pub member: UserId,
    /// The channel key, sealed to `member`'s device public key (hex). Opaque.
    pub sealed_key: String,
}

impl ChannelKeyGrant {
    pub fn new(
        server_id: ServerId,
        channel_id: ChannelId,
        epoch: u32,
        member: UserId,
        sealed_key: impl Into<String>,
    ) -> Self {
        Self {
            server_id,
            channel_id,
            epoch,
            member,
            sealed_key: sealed_key.into(),
        }
    }
}
