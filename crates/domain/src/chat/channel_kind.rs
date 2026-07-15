//! Whether a channel carries text or live voice.

use serde::{Deserialize, Serialize};

/// A channel's kind. Every channel — of either kind — carries the ordinary text
/// message stream, so a [`Voice`](ChannelKind::Voice) channel is a superset of a
/// [`Text`](ChannelKind::Text) one: it *additionally* hosts a live, ephemeral
/// participant roster and full-mesh peer-to-peer WebRTC audio (see
/// `docs/voice-channels.md`). The default is [`Text`](ChannelKind::Text), so
/// pre-voice snapshots load unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ChannelKind {
    /// A text channel — an ordered stream of messages (the default).
    #[default]
    Text,
    /// A voice channel — a live audio room with an ephemeral roster, no messages.
    Voice,
}

impl ChannelKind {
    pub fn is_voice(&self) -> bool {
        matches!(self, ChannelKind::Voice)
    }

    /// The kind's canonical lowercase wire tag — the single home for the string
    /// form, so a new kind is named here rather than at each call site.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Text => "text",
            ChannelKind::Voice => "voice",
        }
    }
}
