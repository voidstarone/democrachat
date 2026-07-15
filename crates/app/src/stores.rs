//! The driven ports the use-cases persist through, bundled into one struct.

use std::sync::Arc;

use crate::{
    BlockStore, ChannelKeyStore, ChannelStore, DmStore, EmojiStore, EmojiVoteStore, FriendStore,
    ImageTranscoder, InviteStore, KeyDirectoryStore, MediaStore, MembershipStore, MessageStore,
    ProposalStore, ReactionStore, RoleColorVoteStore, RoleStore, RuleStore, ServerStore, UserStore,
    VoteStore,
};

/// The driven ports the use-cases persist through. Bundled into one struct so
/// [`Services::new`](crate::Services::new) takes a single argument instead of a
/// dozen — the composition root fills it from whichever store(s) it chose.
#[derive(Clone)]
pub struct Stores {
    pub users: Arc<dyn UserStore>,
    pub servers: Arc<dyn ServerStore>,
    pub memberships: Arc<dyn MembershipStore>,
    pub channels: Arc<dyn ChannelStore>,
    pub messages: Arc<dyn MessageStore>,
    pub reactions: Arc<dyn ReactionStore>,
    pub proposals: Arc<dyn ProposalStore>,
    pub votes: Arc<dyn VoteStore>,
    pub emojis: Arc<dyn EmojiStore>,
    pub emoji_votes: Arc<dyn EmojiVoteStore>,
    pub rules: Arc<dyn RuleStore>,
    pub dms: Arc<dyn DmStore>,
    pub blocks: Arc<dyn BlockStore>,
    pub friends: Arc<dyn FriendStore>,
    pub roles: Arc<dyn RoleStore>,
    pub role_color_votes: Arc<dyn RoleColorVoteStore>,
    pub keys: Arc<dyn KeyDirectoryStore>,
    pub channel_keys: Arc<dyn ChannelKeyStore>,
    pub invites: Arc<dyn InviteStore>,
    pub media: Arc<dyn MediaStore>,
    /// Normalizes uploaded images (re-encode, HEIC→JPEG). Not a persistence port —
    /// a stateless transform — but wired the same way so the codec stays out of
    /// the app core.
    pub image: Arc<dyn ImageTranscoder>,
}
