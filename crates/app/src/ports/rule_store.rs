//! Persistence for a server's rulebook.

use domain::{Rule, RuleId, ServerId};
use crate::StoreError;

/// Persistence for community rules.
pub trait RuleStore: Send + Sync {
    fn next_rule_id(&self) -> Result<RuleId, StoreError>;
    fn insert_rule(&self, rule: Rule) -> Result<(), StoreError>;
    fn remove_rule(&self, id: RuleId) -> Result<bool, StoreError>;
    fn list_for_server(&self, server: ServerId) -> Result<Vec<Rule>, StoreError>;
}
