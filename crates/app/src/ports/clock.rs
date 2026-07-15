//! The source of "now".

use domain::Timestamp;

/// The source of "now". Injected so the governance rules — which are all
/// functions of time — can be tested against a controllable clock.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}
