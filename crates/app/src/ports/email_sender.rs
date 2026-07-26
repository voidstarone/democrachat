//! Driven port: deliver a transactional email (e.g. the signup verification link).

use async_trait::async_trait;

/// Where the web layer hands an outbound email. Like [`VoteRouter`](crate::VoteRouter),
/// this crosses the network (SMTP), so it returns `Result<(), String>` (the web
/// layer logs/surfaces the message) rather than a `StoreError`. The composition
/// root supplies an SMTP adapter; deployments with verification disabled leave it
/// unset.
#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String>;
}
