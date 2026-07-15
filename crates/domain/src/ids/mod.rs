//! Strongly-typed identifiers. Newtypes prevent mixing a `UserId` with a
//! `ServerId` at compile time. Stores assign the underlying values.

pub mod channel_id;
pub mod emoji_id;
pub mod server_id;
pub mod message_id;
pub mod proposal_id;
pub mod report_id;
pub mod rule_id;
pub mod trial_id;
pub mod user_id;
pub mod dm_id;
pub mod role_id;
