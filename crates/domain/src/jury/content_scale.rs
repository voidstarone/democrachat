//! What a panel is judging.

use serde::{Deserialize, Serialize};

/// What a panel is judging. A comment/message is lower-stakes than a top-level
/// post, so it draws a smaller panel; a user-level report (e.g. a suspected bot)
/// is treated as the heavier, post-weight case.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ContentScale {
    Post,
    Comment,
}
