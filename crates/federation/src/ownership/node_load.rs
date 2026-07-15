//! A node's current load, reported to the control plane.

/// A node's current load, reported to the control plane so rehoming can pick the
/// least-loaded target (see [`crate::rehome`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeLoad {
    /// Scopes (servers + user-homes) this node currently owns.
    pub hosted_scopes: u32,
    /// Recent request rate.
    pub requests_per_sec: f64,
}
