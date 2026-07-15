//! Persistence for servers (the self-governing servers).

use domain::{Server, ServerId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for servers (the self-governing servers).
#[async_trait]
pub trait ServerStore: Send + Sync {
    async fn next_server_id(&self) -> Result<ServerId, StoreError>;
    async fn insert_server(&self, server: Server) -> Result<(), StoreError>;
    async fn get_server(&self, id: ServerId) -> Result<Option<Server>, StoreError>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<Server>, StoreError>;
    async fn update_server(&self, server: Server) -> Result<(), StoreError>;
    /// Every server, for a directory listing. Order is unspecified; callers sort.
    async fn list_all(&self) -> Result<Vec<Server>, StoreError>;
}
