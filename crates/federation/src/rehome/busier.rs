//! Ordering two nodes from least to most busy.

use std::cmp::Ordering;

use crate::ownership::node_load::NodeLoad;

/// Order two loads from least to most busy: fewer requests/sec first, then fewer
/// hosted scopes. (`requests_per_sec` is compared with a total order that treats
/// NaN as "most busy", which never occurs in practice.)
pub(super) fn busier(a: &NodeLoad, b: &NodeLoad) -> Ordering {
    a.requests_per_sec
        .partial_cmp(&b.requests_per_sec)
        .unwrap_or(Ordering::Greater)
        .then(a.hosted_scopes.cmp(&b.hosted_scopes))
}
