//! Who may see and post in a channel.

use serde::{Deserialize, Serialize};

/// A channel's visibility class. Most channels are [`Open`](ChannelVisibility::Open)
/// — every member sees them, as chat always has. The [`Appeals`](ChannelVisibility::Appeals)
/// class is the one exception: it backs the per-server `#appeals` channel where a
/// muted member pleads their case, readable only by the muted appellant, the
/// server's voters (citizens), and its police. The access rule itself lives in the
/// app layer (it needs a member's tier and mute state); this enum only tags the
/// channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ChannelVisibility {
    /// Visible to every member (the default for all ordinary channels).
    #[default]
    Open,
    /// The appeals channel — restricted to muted members, citizens, and police.
    Appeals,
}

impl ChannelVisibility {
    pub fn is_appeals(&self) -> bool {
        matches!(self, ChannelVisibility::Appeals)
    }

    /// The visibility's canonical lowercase wire tag — the single home for the
    /// string form, so a new class is named here rather than at each call site.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelVisibility::Open => "open",
            ChannelVisibility::Appeals => "appeals",
        }
    }
}
