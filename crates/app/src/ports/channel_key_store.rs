//! Persistence for encrypted-channel key grants.

use domain::{ChannelId, ChannelKeyGrant, UserId};

/// Stores the sealed channel-key grants — one per `(channel, epoch, member)`. The
/// store treats each `sealed_key` as opaque (it is sealed to a device key it has no
/// secret for); it only files and returns them.
pub trait ChannelKeyStore: Send + Sync {
    /// Publish (or replace) a grant for a `(channel, epoch, member)`.
    fn put_grant(&self, grant: ChannelKeyGrant);
    /// Every grant a member holds in a channel — one per epoch they've been granted.
    fn grants_for_member(&self, channel: ChannelId, member: UserId) -> Vec<ChannelKeyGrant>;
}
