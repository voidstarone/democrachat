//! Tags: a small, freely-chosen label set on a server, channel, or user.
//!
//! Stored as ONE pipe-fenced string so an exact-tag test is a substring match —
//! see [`tags::Tags`](tags::Tags).

pub mod tags;
