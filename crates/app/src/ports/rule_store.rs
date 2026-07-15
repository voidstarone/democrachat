//! Persistence for a server's rulebook.

use domain::{Rule, RuleId, ServerId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for community rules.
#[async_trait]
pub trait RuleStore: Send + Sync {
    async fn next_rule_id(&self) -> Result<RuleId, StoreError>;
    async fn insert_rule(&self, rule: Rule) -> Result<(), StoreError>;
    async fn remove_rule(&self, id: RuleId) -> Result<bool, StoreError>;
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Rule>, StoreError>;
}
