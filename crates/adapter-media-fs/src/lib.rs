//! A filesystem-backed [`MediaStore`] — the media storage tier.
//!
//! Media is a **separate storage concern** from the federated shard data: blobs
//! live as plain files under a media directory (a dedicated media node in a real
//! deployment; the same box, different directory, in a single-box one), never in
//! the JSON snapshot or the replication feed. Each blob is a pair of files —
//! `<key>` (the bytes) and `<key>.ct` (its MIME type) — under an opaque, unguessable
//! hex key. Deleting a message deletes its blobs (see [`MediaStore::delete`]).

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use app::{MediaError, MediaStore};

/// Stores media blobs as files in `dir`.
pub struct FsMediaStore {
    dir: PathBuf,
    seq: AtomicU64,
}

impl FsMediaStore {
    /// Open (creating if needed) the media directory.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir, seq: AtomicU64::new(0) })
    }

    /// Mint a fresh 128-bit opaque hex key. Not content-addressed — every upload
    /// gets a distinct key, so deleting one message's media never affects another's.
    fn mint_key(&self, bytes: &[u8]) -> String {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let mut h1 = DefaultHasher::new();
        (seq, nanos, bytes.len()).hash(&mut h1);
        bytes.get(..bytes.len().min(64)).hash(&mut h1);
        let a = h1.finish();
        let mut h2 = DefaultHasher::new();
        (a, nanos, seq, bytes.len()).hash(&mut h2);
        format!("{a:016x}{:016x}", h2.finish())
    }

    /// Resolve a key to its blob path, rejecting anything that is not pure hex
    /// (guards against path traversal — minted keys are always hex).
    fn path_for(&self, key: &str) -> Option<PathBuf> {
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.dir.join(key))
    }
}

impl MediaStore for FsMediaStore {
    fn put(&self, content_type: &str, bytes: &[u8]) -> Result<String, MediaError> {
        let key = self.mint_key(bytes);
        let path = self.dir.join(&key);
        fs::write(&path, bytes).map_err(|_| MediaError::Io)?;
        fs::write(path.with_extension("ct"), content_type.as_bytes()).map_err(|_| MediaError::Io)?;
        Ok(key)
    }

    fn get(&self, key: &str) -> Option<(String, Vec<u8>)> {
        let path = self.path_for(key)?;
        let bytes = fs::read(&path).ok()?;
        let content_type = fs::read_to_string(path.with_extension("ct"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "application/octet-stream".to_string());
        Some((content_type, bytes))
    }

    fn delete(&self, key: &str) {
        if let Some(path) = self.path_for(key) {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("ct"));
        }
    }
}
