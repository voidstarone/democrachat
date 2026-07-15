//! Persistence for servers (the self-governing servers).

use domain::{Server, ServerId};

/// Persistence for servers (the self-governing servers).
pub trait ServerStore: Send + Sync {
    fn next_server_id(&self) -> ServerId;
    fn insert_server(&self, server: Server);
    fn get_server(&self, id: ServerId) -> Option<Server>;
    fn find_by_slug(&self, slug: &str) -> Option<Server>;
    fn update_server(&self, server: Server);
    /// Every server, for a directory listing. Order is unspecified; callers sort.
    fn list_all(&self) -> Vec<Server>;
}
