//! Pick a fresh standby after a promotion.

use domain::NodeId;

use crate::ownership::node_status::NodeStatus;

use super::busier::busier;

/// Pick a fresh standby after a promotion: the live, lowest-traffic node not in
/// `exclude` (which holds the new owner and any node to skip).
pub fn choose_new_standby(exclude: &[NodeId], loads: &[NodeStatus]) -> Option<NodeId> {
    loads
        .iter()
        .filter(|s| !exclude.contains(&s.node))
        .min_by(|a, b| busier(&a.load, &b.load).then(a.node.0.cmp(&b.node.0)))
        .map(|s| s.node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::node_load::NodeLoad;

    fn status(node: u16, rps: f64, hosted: u32) -> NodeStatus {
        NodeStatus {
            node: NodeId(node),
            load: NodeLoad {
                hosted_scopes: hosted,
                requests_per_sec: rps,
            },
        }
    }

    #[test]
    fn fresh_standby_excludes_the_new_owner() {
        let loads = [status(2, 1.0, 0), status(3, 5.0, 0)];
        assert_eq!(
            choose_new_standby(&[NodeId(2)], &loads),
            Some(NodeId(3)),
            "the new owner (2) is excluded, so the quiet remaining node is chosen"
        );
    }
}
