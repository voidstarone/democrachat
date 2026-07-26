//! A server — a self-governing community (the democrachat unit of governance).

// One-def-per-file: this directory groups server defs; `server.rs` holds `Server` itself.
#[allow(clippy::module_inception)]
pub mod server;
pub mod phase;
pub mod phase_thresholds;
pub mod slugify;
pub mod invite;
pub mod invite_policy;
