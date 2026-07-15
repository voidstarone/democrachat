//! A text channel within a server.

use serde::{Deserialize, Serialize};

use crate::chat::channel_kind::ChannelKind;
use crate::chat::channel_visibility::ChannelVisibility;
use crate::chat::history_mode::HistoryMode;
use crate::{ChannelId, ServerId, Tags, Timestamp};

/// A text channel — an ordered stream of messages within a server. Whether a
/// channel comes into being by founder provisioning (Seed) or by a ballot
/// (Chartering+) is decided in `app`; the entity itself is inert.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub server_id: ServerId,
    /// Normalized name (see [`crate::normalize_channel_name`]) — unique per server.
    pub name: String,
    pub topic: String,
    pub created_at: Timestamp,
    /// Text, or Voice (which *adds* a live audio room on top of the same message
    /// stream). `#[serde(default)]` = [`ChannelKind::Text`], so pre-voice datasets load.
    #[serde(default)]
    pub kind: ChannelKind,
    /// Whether message bodies in this channel are end-to-end encrypted under a
    /// channel key. Default `false` (plaintext, node-readable). Turning it on is a
    /// deliberate, client-driven act — the server never holds the channel key.
    #[serde(default)]
    pub is_encrypted: bool,
    /// How the channel key reaches members — only meaningful when encrypted.
    #[serde(default)]
    pub history_mode: HistoryMode,
    /// Who may see and post in this channel. Defaults to
    /// [`ChannelVisibility::Open`] (every member), and older datasets read that
    /// way too; the restricted `#appeals` channel is the sole exception.
    #[serde(default)]
    pub visibility: ChannelVisibility,
    /// Free-form discovery tags for this channel. Empty by default;
    /// `#[serde(default)]` keeps pre-tags datasets loadable.
    #[serde(default)]
    pub tags: Tags,
}

impl Channel {
    pub fn new(
        id: ChannelId,
        server_id: ServerId,
        name: impl Into<String>,
        topic: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            server_id,
            name: name.into(),
            topic: topic.into(),
            created_at,
            kind: ChannelKind::Text,
            is_encrypted: false,
            history_mode: HistoryMode::Open,
            visibility: ChannelVisibility::Open,
            tags: Tags::default(),
        }
    }

    /// A restricted `#appeals` channel — where muted members plead their case to
    /// the server's voters. Same as [`new`](Self::new) but tagged
    /// [`ChannelVisibility::Appeals`].
    pub fn appeals(
        id: ChannelId,
        server_id: ServerId,
        name: impl Into<String>,
        topic: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        let mut c = Self::new(id, server_id, name, topic, created_at);
        c.visibility = ChannelVisibility::Appeals;
        c
    }

    /// A voice channel — a live audio room with no message stream. Same as
    /// [`new`](Self::new) but tagged [`ChannelKind::Voice`].
    pub fn voice(
        id: ChannelId,
        server_id: ServerId,
        name: impl Into<String>,
        topic: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        let mut c = Self::new(id, server_id, name, topic, created_at);
        c.kind = ChannelKind::Voice;
        c
    }

    /// Turn on end-to-end encryption for this channel with the given history mode.
    /// The channel key itself is created and distributed client-side; this only
    /// flips the channel's stored policy.
    pub fn enable_encryption(&mut self, history_mode: HistoryMode) {
        self.is_encrypted = true;
        self.history_mode = history_mode;
    }
}
