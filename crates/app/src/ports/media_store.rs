//! Driven port: opaque blob storage for message media attachments.

use crate::MediaError;

/// Where message media (images, video, audio) is stored. Keys are opaque and
/// globally addressable — deliberately independent of a server's federation
/// shard, so media can live on its own storage tier (a dedicated media node)
/// rather than travelling the control/replication plane. The bytes are served
/// verbatim at `/media/{key}` with the stored content type.
pub trait MediaStore: Send + Sync {
    /// Store `bytes` under a freshly minted opaque key and return it. `content_type`
    /// is persisted alongside so the blob can later be served with the right type.
    /// Each call mints a distinct key (no cross-message sharing), so deleting one
    /// message's media never affects another's.
    fn put(&self, content_type: &str, bytes: &[u8]) -> Result<String, MediaError>;

    /// Fetch a blob and its content type by key, or `None` if unknown.
    fn get(&self, key: &str) -> Option<(String, Vec<u8>)>;

    /// Delete a blob. Idempotent — an unknown key is a no-op.
    fn delete(&self, key: &str);
}
