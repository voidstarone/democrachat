//! Trial by jury — the fast path for content moderation (spam, abuse) where a
//! multi-day server ballot would be absurd.
//!
//! When a report goes to trial, a jury of existing members is empanelled at
//! random and votes on guilt. Two design choices carry the fairness of the
//! system:
//!
//! * **Selection is deterministic given a seed** — no hidden RNG state — so a
//!   jury can be reproduced and audited. (The application supplies a seed, e.g.
//!   derived from the report id.)
//! * **Conviction requires a supermajority of the *whole* jury** (default 2/3),
//!   protecting the accused, mirroring "supermajority protects minorities".

pub mod content_scale;
pub mod default_jury_size;
pub mod jury_ballot;
pub mod jury_sizing;
pub mod reach_verdict;
pub mod select_jury;
pub mod trial;
pub mod verdict;
