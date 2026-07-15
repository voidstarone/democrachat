//! Use-cases for encrypted channels: turn on encryption, publish a member's key
//! grant, and fetch one's own grants.
//!
//! The server is **blind** to the channel key. It stores the per-member sealed
//! grants and message ciphertext opaquely; the key is generated and distributed
//! entirely client-side. These use-cases move opaque blobs and enforce membership —
//! they never see a key or a plaintext body.

use domain::{ChannelKeyGrant, HistoryMode};

use crate::error::channel_key_error::ChannelKeyError;
use crate::Services;

impl Services {
    /// Turn on end-to-end encryption for a channel, with the given history mode
    /// (citizen-only). This only flips the channel's stored policy — the channel key
    /// is minted and granted to members client-side afterwards.
    pub fn enable_channel_encryption(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        history_mode: HistoryMode,
    ) -> Result<(), ChannelKeyError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchServer(server_slug.to_string()))?;
        self.memberships
            .get(user.id, server.id)
            .filter(|m| m.is_franchised())
            .ok_or(ChannelKeyError::NotACitizen)?;

        let name = domain::normalize_channel_name(channel_name);
        let mut channel = self
            .channels
            .find_by_name(server.id, &name)
            .ok_or_else(|| ChannelKeyError::NoSuchChannel(channel_name.to_string()))?;
        channel.enable_encryption(history_mode);
        self.channels.insert_channel(channel);
        Ok(())
    }

    /// Publish a sealed channel-key grant for `member` at `epoch`. The caller (a
    /// member who holds the key) has sealed the key to the member's device key; the
    /// server stores the blob. Both the granter and the grantee must be members.
    pub fn grant_channel_key(
        &self,
        granter_handle: &str,
        server_slug: &str,
        channel_name: &str,
        epoch: u32,
        member_handle: &str,
        sealed_key: &str,
    ) -> Result<(), ChannelKeyError> {
        let granter = self
            .users
            .find_by_handle(granter_handle.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchUser(granter_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchServer(server_slug.to_string()))?;
        self.memberships
            .get(granter.id, server.id)
            .ok_or_else(|| ChannelKeyError::NotAMember(granter_handle.to_string()))?;

        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name)
            .ok_or_else(|| ChannelKeyError::NoSuchChannel(channel_name.to_string()))?;
        if !channel.is_encrypted {
            return Err(ChannelKeyError::NotEncrypted);
        }

        let member = self
            .users
            .find_by_handle(member_handle.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchUser(member_handle.to_string()))?;
        self.memberships
            .get(member.id, server.id)
            .ok_or_else(|| ChannelKeyError::NotAMember(member_handle.to_string()))?;

        let sealed_key = sealed_key.trim();
        if sealed_key.is_empty() {
            return Err(ChannelKeyError::EmptySeal);
        }
        self.channel_keys.put_grant(ChannelKeyGrant::new(
            server.id,
            channel.id,
            epoch,
            member.id,
            sealed_key,
        ));
        Ok(())
    }

    /// The caller's own key grants in a channel — one per epoch they've been given —
    /// so their client can open messages sealed under those epochs.
    pub fn my_channel_grants(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<Vec<ChannelKeyGrant>, ChannelKeyError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| ChannelKeyError::NoSuchServer(server_slug.to_string()))?;
        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name)
            .ok_or_else(|| ChannelKeyError::NoSuchChannel(channel_name.to_string()))?;
        Ok(self.channel_keys.grants_for_member(channel.id, user.id))
    }
}
