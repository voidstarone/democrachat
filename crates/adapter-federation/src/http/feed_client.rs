//! An HTTP client for one peer's change feed.

use federation::ChangeEvent;

/// Pulls signed change events from a single peer's feed endpoint.
pub struct FeedClient {
    base_url: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl FeedClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token,
            http: reqwest::Client::new(),
        }
    }

    /// Fetch at most `limit` events with `seq > since` (oldest first). The events
    /// are returned unverified — [`Replicator::ingest`](crate::Replicator::ingest)
    /// authorizes them before anything is applied.
    pub async fn changes_since(&self, since: u64, limit: u64) -> Result<Vec<ChangeEvent>, String> {
        let url = format!(
            "{}/federation/changes?since={since}&limit={limit}",
            self.base_url.trim_end_matches('/')
        );
        let mut req = self.http.get(url);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("peer feed returned HTTP {}", resp.status().as_u16()));
        }
        resp.json().await.map_err(|e| e.to_string())
    }
}
