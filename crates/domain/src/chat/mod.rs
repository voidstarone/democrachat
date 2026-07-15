//! The chat model: channels, messages (with threaded replies), and reactions.
//!
//! Structure only lives here — *who* may create a channel or post is a governance
//! question answered in `app`/`domain::governance`, not in these entities.

pub mod attachment;
pub mod build_message_tree;
pub mod channel;
pub mod channel_kind;
pub mod channel_visibility;
pub mod media_kind;
pub mod channel_key_grant;
pub mod history_mode;
pub mod message;
pub mod message_node;
pub mod normalize_channel_name;
pub mod reaction;
pub mod parse_mentions;
