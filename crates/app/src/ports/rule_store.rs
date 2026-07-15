//! Persistence for a server's rulebook.

use domain::{Rule, RuleId, ServerId};

/// Persistence for community rules.
pub trait RuleStore: Send + Sync {
    fn next_rule_id(&self) -> RuleId;
    fn insert_rule(&self, rule: Rule);
    fn remove_rule(&self, id: RuleId) -> bool;
    fn list_for_server(&self, server: ServerId) -> Vec<Rule>;
}
