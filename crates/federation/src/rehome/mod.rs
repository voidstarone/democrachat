//! Failover and load-aware rehoming.
//!
//! When a scope's owner node goes down, its ownership **lease lapses** and the
//! scope becomes unowned (`owner_of` → `None`). A peer then takes over — but only
//! a node that already holds a caught-up replica may, or writes would be lost. The
//! designated **standbys** are exactly those pre-warmed nodes, so failover
//! promotes a standby. The [`claim`](crate::ownership::OwnershipRegistry::claim)
//! that does so bumps the epoch, fencing the old owner if it returns.
//!
//! Among eligible standbys the **lowest-traffic** one is chosen, so a scope
//! rehomes onto a quiet node rather than piling onto a busy one — the
//! load-balancing the deployment asked for. The winner then designates a fresh
//! standby (again the quietest live node) so the scope is once more protected.
//!
//! The placement decisions are pure functions
//! ([`choose_new_owner`](choose_new_owner::choose_new_owner),
//! [`choose_new_standby`](choose_new_standby::choose_new_standby)) so they are
//! exhaustively testable;
//! [`RehomingController`](rehoming_controller::RehomingController) ties them to the
//! control plane.

mod busier;
pub mod choose_new_owner;
pub mod choose_new_standby;
pub mod rehome_outcome;
pub mod rehoming_controller;

pub use choose_new_owner::choose_new_owner;
pub use choose_new_standby::choose_new_standby;
pub use rehome_outcome::RehomeOutcome;
pub use rehoming_controller::RehomingController;
