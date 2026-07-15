//! The social layer: direct messages, blocks, and friendships.
//!
//! DMs are platform-wide (they belong to no server). A user's [`DmPolicy`]
//! decides who may reach them; a [`Block`] overrides that in both directions and
//! is permanent; a [`Friendship`] backs the friends-only policy. The single
//! gatekeeping rule lives in [`can_dm`].

pub mod block;
pub mod can_dm;
pub mod dm_message;
pub mod dm_policy;
pub mod friend_status;
pub mod friendship;
