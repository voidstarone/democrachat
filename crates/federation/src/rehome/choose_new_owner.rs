//! Pick the new owner for an unowned scope.

use domain::NodeId;

use crate::ownership::node_status::NodeStatus;

use super::busier::busier;

/// Pick the new owner for an unowned scope: the **live, lowest-traffic standby**.
/// A standby that hasn't reported load (not live) is skipped — we never promote a
/// node we can't confirm is up. `None` means no eligible standby.
pub fn choose_new_owner(standbys: &[NodeId], loads: &[NodeStatus]) -> Option<NodeId> {
    standbys
        .iter()
        .filter_map(|&sb| loads.iter().find(|s| s.node == sb).copied())
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
    fn new_owner_is_the_quietest_live_standby() {
        let loads = [status(2, 10.0, 5), status(3, 2.0, 1), status(4, 2.0, 9)];
        // Standbys 2 and 3; node 3 is quieter → it wins.
        assert_eq!(choose_new_owner(&[NodeId(2), NodeId(3)], &loads), Some(NodeId(3)));
        // Tie on rps (3 vs 4) is broken by hosted_scopes, then id.
        assert_eq!(choose_new_owner(&[NodeId(3), NodeId(4)], &loads), Some(NodeId(3)));
    }

    #[test]
    fn a_standby_that_is_not_live_is_not_promoted() {
        let loads = [status(2, 5.0, 0)];
        // Node 9 is a standby but reported no load (down) → skipped; node 2 wins.
        assert_eq!(choose_new_owner(&[NodeId(9), NodeId(2)], &loads), Some(NodeId(2)));
        // No live standby at all → nobody.
        assert_eq!(choose_new_owner(&[NodeId(9)], &loads), None);
    }
}
