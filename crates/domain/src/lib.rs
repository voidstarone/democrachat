//! democrachat domain core — pure governance logic for a self-governing chat
//! platform.
//!
//! This crate has no knowledge of databases, HTTP, the filesystem, or any async
//! runtime. Every rule is a pure function of its inputs (entities + a `now`
//! timestamp), which makes the governance engine exhaustively unit-testable and
//! keeps the four defensive layers in one auditable place:
//!
//! * Layer 1 — earned franchise: [`franchise::evaluate_eligibility`]
//! * Layer 2 — enfranchisement rate cap: [`franchise::enfranchisement_slots`]
//! * Layer 3 — tiered thresholds: [`governance::threshold_for`] + [`governance::decide`]
//! * Layer 4 — timelock + recall: [`governance::Proposal::close`]
//!
//! ## The hard invariant
//!
//! Becoming a [`Citizen`](membership::tier::Tier::Citizen) — the tier that may
//! vote — is possible **only** by meeting a server's [`FranchiseCriteria`], as
//! judged by [`evaluate_eligibility`]. There is no proposal, role, admin action,
//! or invite anywhere in this crate that grants the franchise. Weighting a vote
//! ([`ProposalKind::GrantVoteWeight`]) only ever adjusts an *already*-enfranchised
//! citizen's ballot.

pub mod chat;
pub mod credentials;
pub mod emoji;
pub mod emoji_image;
pub mod emoji_ranking;
pub mod emoji_vote;
pub mod franchise;
pub mod governance;
pub mod server;
pub mod ids;
pub mod jury;
pub mod keys;
pub mod membership;
pub mod node;
pub mod roles;
pub mod rules;
pub mod social;
pub mod time;
pub mod user;
pub mod weighting;

pub use chat::attachment::Attachment;
pub use chat::build_message_tree::build_message_tree;
pub use chat::channel::Channel;
pub use chat::media_kind::MediaKind;
pub use chat::channel_key_grant::ChannelKeyGrant;
pub use chat::history_mode::HistoryMode;
pub use chat::message::Message;
pub use chat::message_node::MessageNode;
pub use chat::normalize_channel_name::normalize_channel_name;
pub use chat::parse_mentions::parse_mentions;
pub use chat::reaction::{summarize_reactions, Reaction};

pub use franchise::eligibility::Eligibility;
pub use franchise::enfranchisement_slots::enfranchisement_slots;
pub use franchise::evaluate_eligibility::evaluate_eligibility;
pub use franchise::franchise_criteria::FranchiseCriteria;
pub use franchise::unmet::Unmet;

pub use governance::ballot_kind::BallotKind;
pub use governance::decide::decide;
pub use governance::decision::Decision;
pub use governance::decision_class::DecisionClass;
pub use governance::proposal::Proposal;
pub use governance::proposal_kind::ProposalKind;
pub use governance::proposal_status::ProposalStatus;
pub use governance::recall_window_days::RECALL_WINDOW_DAYS;
pub use governance::tally::Tally;
pub use governance::threshold::Threshold;
pub use governance::threshold_for::threshold_for;
pub use governance::vote::Vote;

pub use emoji::Emoji;
pub use emoji_vote::EmojiVote;
pub use emoji_image::emoji_image_error::EmojiImageError;
pub use emoji_image::emoji_image_format::EmojiImageFormat;
pub use emoji_image::validate_emoji_image::{
    validate_emoji_image, MAX_EMOJI_IMAGE_BYTES, MAX_EMOJI_IMAGE_DIMENSION,
};
pub use emoji_ranking::emoji_slots::{ACTIVE_EMOJI_SLOTS, CONSIDERED_EMOJI_SLOTS};
pub use emoji_ranking::emoji_standing::EmojiStanding;
pub use emoji_ranking::normalize_emoji_name::normalize_emoji_name;
pub use emoji_ranking::rank_emojis::rank_emojis;

pub use credentials::email_error::EmailError;
pub use credentials::max_email_len::MAX_EMAIL_LEN;
pub use credentials::max_password_len::MAX_PASSWORD_LEN;
pub use credentials::min_password_len::MIN_PASSWORD_LEN;
pub use credentials::password_error::PasswordError;
pub use credentials::validate_email::validate_email;
pub use credentials::validate_password::validate_password;

pub use server::server::Server;
pub use server::phase::Phase;
pub use server::slugify::slugify;
pub use server::invite::Invite;
pub use server::invite_policy::InvitePolicy;

pub use ids::channel_id::ChannelId;
pub use ids::dm_id::DmId;
pub use ids::emoji_id::EmojiId;
pub use ids::server_id::ServerId;
pub use ids::message_id::MessageId;
pub use ids::proposal_id::ProposalId;
pub use ids::report_id::ReportId;
pub use ids::role_id::RoleId;
pub use ids::rule_id::RuleId;
pub use ids::trial_id::TrialId;
pub use ids::user_id::UserId;

pub use jury::content_scale::ContentScale;
pub use jury::default_jury_size::DEFAULT_JURY_SIZE;
pub use jury::jury_ballot::JuryBallot;
pub use jury::jury_sizing::JurySizing;
pub use jury::reach_verdict::reach_verdict;
pub use jury::select_jury::select_jury;
pub use jury::trial::Trial;
pub use jury::verdict::Verdict;

pub use membership::membership::Membership;
pub use membership::tier::Tier;

pub use node::compose_id::compose_id;
pub use node::local_sequence::local_sequence;
pub use node::max_sequence::MAX_SEQUENCE;
pub use node::node_id::NodeId;
pub use node::origin_node::origin_node;
pub use node::sequence_bits::SEQUENCE_BITS;
pub use node::sequence_mask::SEQUENCE_MASK;

pub use roles::normalize_role_name::normalize_role_name;
pub use roles::role::Role;
pub use roles::role_assignment::RoleAssignment;
pub use roles::standing_role::StandingRole;

pub use rules::Rule;

pub use keys::user_keys::UserKeys;
pub use keys::wrapped_key::WrappedKey;

pub use social::block::Block;
pub use social::can_dm::can_dm;
pub use social::dm_message::DmMessage;
pub use social::dm_policy::DmPolicy;
pub use social::friend_status::FriendStatus;
pub use social::friendship::Friendship;

pub use time::Timestamp;
pub use user::user::User;

pub use weighting::max_vote_weight::MAX_VOTE_WEIGHT;
pub use weighting::vote_weighting::VoteWeighting;
pub use weighting::weighting_scope::WeightingScope;
