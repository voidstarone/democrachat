//! democrachat application layer — use-cases and the port traits adapters
//! implement. Depends only on `domain`; names no database or delivery mechanism.

pub mod auth;
pub mod channel_key_service;
pub mod chat_service;
pub mod e2ee;
pub mod emoji_service;
pub mod emoji_view;
pub mod error;
pub mod governance_service;
pub mod invite;
pub mod key_directory_service;
pub mod member_view;
pub mod mention_kind;
pub mod mute_service;
pub mod outcome;
pub mod passthrough_transcoder;
pub mod ports;
pub mod resolved_mention;
pub mod role_color_view;
pub mod role_service;
pub mod services;
pub mod session;
pub mod social_service;
pub mod stores;
pub mod user_roles;
pub mod vault;

pub use error::channel_error::ChannelError;
pub use error::channel_key_error::ChannelKeyError;
pub use error::dm_error::DmError;
pub use error::emoji_error::EmojiError;
pub use error::enfranchise_error::EnfranchiseError;
pub use error::found_error::FoundError;
pub use error::invite_error::InviteError;
pub use error::join_error::JoinError;
pub use error::key_error::KeyError;
pub use error::media_error::MediaError;
pub use error::message_error::MessageError;
pub use error::mute_error::MuteError;
pub use error::propose_error::ProposeError;
pub use error::reaction_error::ReactionError;
pub use error::register_error::RegisterError;
pub use error::role_error::RoleError;
pub use error::social_error::SocialError;
pub use error::store_error::StoreError;
pub use error::vote_error::VoteError;

pub use auth::hash_password::hash_password;
pub use auth::spend_verify_time::spend_verify_time;
pub use auth::verify_password::verify_password;

pub use session::constant_time_eq::constant_time_eq;
pub use session::session_signer::SessionSigner;

pub use vault::open::open as vault_open;
pub use vault::seal::seal as vault_seal;
pub use vault::sealed::Sealed;
pub use vault::vault_error::VaultError;
pub use vault::vault_key::VaultKey;

pub use e2ee::channel_key::ChannelKey;
pub use e2ee::e2ee_error::E2eeError;
pub use e2ee::identity_secret::IdentitySecret;
pub use e2ee::open::open_sealed;
pub use e2ee::open_message::open_channel_message;
pub use e2ee::seal_message::seal_channel_message;
pub use e2ee::public_identity::PublicIdentity;
pub use e2ee::seal::seal_to;
pub use e2ee::unwrap::unwrap_secret;
pub use e2ee::wrap::wrap_secret;
pub use e2ee::wrapped_secret::WrappedSecret;

pub use mention_kind::MentionKind;
pub use resolved_mention::ResolvedMention;
pub use member_view::MemberView;

pub use outcome::EnfranchiseOutcome;

pub use ports::block_store::BlockStore;
pub use ports::channel_key_store::ChannelKeyStore;
pub use ports::channel_store::ChannelStore;
pub use ports::clock::Clock;
pub use ports::block_router::BlockRouter;
pub use ports::dm_router::DmRouter;
pub use ports::friend_router::FriendRouter;
pub use ports::dm_store::DmStore;
pub use emoji_view::RankedEmoji;
pub use role_color_view::RoleColorView;
pub use user_roles::UserRoles;
pub use ports::emoji_store::EmojiStore;
pub use ports::emoji_vote_store::EmojiVoteStore;
pub use ports::friend_store::FriendStore;
pub use ports::invite_store::InviteStore;
pub use ports::image_transcoder::ImageTranscoder;
pub use ports::key_directory_store::KeyDirectoryStore;
pub use ports::media_store::MediaStore;
pub use passthrough_transcoder::PassthroughTranscoder;
pub use ports::membership_store::{CapAdmission, MembershipStore};
pub use ports::message_store::MessageStore;
pub use ports::proposal_store::ProposalStore;
pub use ports::reaction_store::ReactionStore;
pub use ports::role_color_vote_store::RoleColorVoteStore;
pub use ports::role_store::RoleStore;
pub use ports::rule_store::RuleStore;
pub use ports::server_store::ServerStore;
pub use ports::user_store::UserStore;
pub use ports::vote_router::VoteRouter;
pub use ports::vote_store::VoteStore;

pub use channel_key_service::ChannelKeyService;
pub use chat_service::ChatService;
pub use emoji_service::EmojiService;
pub use governance_service::GovernanceService;
pub use key_directory_service::KeyDirectoryService;
pub use mute_service::MuteService;
pub use role_service::RoleService;
pub use services::Services;
pub use social_service::SocialService;
pub use stores::Stores;
