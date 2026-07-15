//! Invite-code minting and hashing. The raw code is a high-entropy secret shared
//! out of band; only its digest is ever persisted (see [`hash_code`]).

pub mod hash_code;
pub mod new_invite_code;
