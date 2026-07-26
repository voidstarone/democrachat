//! Driven port: deliver a transactional email (e.g. the signup verification link).

use async_trait::async_trait;

/// Where the web layer hands an outbound email. The single async port alongside
/// [`VoteRouter`](crate::VoteRouter) — sending crosses the network (SMTP), so it
/// cannot be a synchronous store port. The composition root supplies an SMTP
/// adapter; deployments with verification disabled leave it unset. Any failure
/// collapses to a message string the web layer logs/surfaces.
#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String>;
}
