//! The weight-classes of a governance decision.

use serde::{Deserialize, Serialize};

/// The weight-classes of decision, in rough ascending order of how hard they are
/// to pass. The class — not the specific action — determines the threshold, so a
/// server's Discord-native ballots (emoji, channels, timeouts) inherit sane bars
/// automatically.
///
/// The variants are declared strictest-last, so the derived [`Ord`] ranks them by
/// difficulty: a **bundled** proposal (an original change plus amendments) takes
/// the `max` of its parts' classes, so the whole bundle clears the hardest bar any
/// one change would demand and — if any part is constitutional — inherits the
/// timelock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum DecisionClass {
    /// Routine, reversible moderation: remove a message, resolve a report, pin.
    Moderation,
    /// Add or repeal a rule, add/remove an emoji, restructure channels. Allowed
    /// in every phase (including Seed), so a founding community can set itself up.
    RuleChange,
    /// Bans, timeouts, leader recall — sanctions against a person.
    BanOrRecall,
    /// Change the franchise criteria, vote weighting, or these very thresholds.
    Constitutional,
}
