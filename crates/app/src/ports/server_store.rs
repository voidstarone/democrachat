//! Persistence for servers (the self-governing servers).

use domain::{Server, ServerId};
use crate::StoreError;

/// Persistence for servers (the self-governing servers).
pub trait ServerStore: Send + Sync {
    fn next_server_id(&self) -> Result<ServerId, StoreError>;
    fn insert_server(&self, server: Server) -> Result<(), StoreError>;
    fn get_server(&self, id: ServerId) -> Result<Option<Server>, StoreError>;
    fn find_by_slug(&self, slug: &str) -> Result<Option<Server>, StoreError>;
    fn update_server(&self, server: Server) -> Result<(), StoreError>;
    /// Every server, for a directory listing. Order is unspecified; callers sort.
    fn list_all(&self) -> Result<Vec<Server>, StoreError>;
}
