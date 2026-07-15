//! An HTTP client for forwarding a signed command to a scope's owner.

use crate::command::forward_error::ForwardError;
use crate::command::signed_command::SignedCommand;

/// Forwards signed commands to one owner node's command endpoint.
pub struct CommandClient {
    base_url: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl CommandClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token,
            http: reqwest::Client::new(),
        }
    }

    /// POST `signed` to the owner and translate the response. A non-2xx status maps
    /// to the matching [`ForwardError`] so the caller can distinguish a domain
    /// rejection (never retry) from a transient owner problem (retry).
    pub async fn forward(&self, signed: &SignedCommand) -> Result<(), ForwardError> {
        let url = format!("{}/federation/command", self.base_url.trim_end_matches('/'));
        let mut req = self.http.post(url).json(signed);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| ForwardError::OwnerUnreachable(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let detail = resp.text().await.unwrap_or_default();
        Err(match status.as_u16() {
            409 => ForwardError::Unowned,
            502..=504 => ForwardError::OwnerUnreachable(format!("HTTP {status}")),
            _ => ForwardError::Rejected(if detail.is_empty() {
                format!("HTTP {status}")
            } else {
                detail
            }),
        })
    }
}
