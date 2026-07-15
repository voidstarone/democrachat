//! A control-plane backend failure.

/// A control-plane backend failure (e.g. etcd unreachable). The in-memory
/// registry never fails, so it is an adapter concern; the trait surfaces it so
/// callers handle a partitioned control plane rather than panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError(pub String);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "control-plane error: {}", self.0)
    }
}

impl std::error::Error for RegistryError {}
