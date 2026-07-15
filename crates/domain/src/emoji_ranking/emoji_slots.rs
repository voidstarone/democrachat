//! How many emoji occupy each tier of a server's ranked pool.

/// The top-ranked emoji a server keeps **active** — offered in the picker and
/// usable in new messages.
pub const ACTIVE_EMOJI_SLOTS: usize = 128;

/// The next tier — **considered** candidates, shown in the vote list so members can
/// promote them, but not yet in the everyday picker.
pub const CONSIDERED_EMOJI_SLOTS: usize = 128;
