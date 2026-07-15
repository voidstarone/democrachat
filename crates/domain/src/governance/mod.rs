//! Layers 3 & 4 of the defense: tiered decision thresholds (how hard a decision
//! is to pass) and the timelock (a passed constitutional change does not take
//! effect immediately, leaving a recall window).
//!
//! This module also carries the **governance surface** ([`ballot_kind`]): the
//! set of things a given server has chosen to put to a vote. One server may only
//! vote on bans; another on which custom emojis exist; another on its whole
//! channel layout. Every [`ProposalKind`](proposal_kind::ProposalKind) maps to a
//! [`BallotKind`](ballot_kind::BallotKind) discriminant, and a server enables the
//! subset it governs democratically.

pub mod ballot_kind;
pub mod decide;
pub mod decision;
pub mod decision_class;
pub mod discussion_post;
pub mod proposal;
pub mod proposal_kind;
pub mod proposal_status;
pub mod recall_window_days;
pub mod tally;
pub mod threshold;
pub mod threshold_for;
pub mod vote;
