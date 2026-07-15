//! A filesystem-backed [`MediaStore`] — the media storage tier.
//!
//! Media is a **separate storage concern** from the federated shard data: blobs
//! live as plain files under a media directory (a dedicated media node in a real
//! deployment; the same box, different directory, in a single-box one), never in
//! the JSON snapshot or the replication feed. Each blob is a pair of files —
//! `<key>` (the bytes) and `<key>.ct` (its MIME type) — under an opaque, unguessable
//! hex key. Deleting a message deletes its blobs (see [`MediaStore::delete`]).

use std::fs;
use std::path::PathBuf;

use app::{MediaError, MediaStore};

/// Stores media blobs as files in `dir`.
pub struct FsMediaStore {
    dir: PathBuf,
}

impl FsMediaStore {
    /// Open (creating if needed) the media directory.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Mint a fresh 128-bit opaque hex key from OS randomness. The key is the sole
    /// capability that reaches a blob, so it must be unguessable — drawn from a
    /// CSPRNG, never derived from the (predictable) upload metadata or content. Not
    /// content-addressed: every upload gets a distinct key, so deleting one
    /// message's media never affects another's.
    fn mint_key() -> String {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).expect("OS randomness for media key");
        hex::encode(bytes)
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
        let key = Self::mint_key();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway media directory under the OS temp dir, unique per test and
    /// removed on drop. The suffix is drawn from OS randomness so parallel test
    /// runs never collide.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new() -> Self {
            let mut suffix = [0u8; 8];
            getrandom::getrandom(&mut suffix).unwrap();
            let dir = std::env::temp_dir().join(format!("democrachat-media-test-{}", hex::encode(suffix)));
            Self(dir)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn store() -> (FsMediaStore, TmpDir) {
        let tmp = TmpDir::new();
        (FsMediaStore::new(&tmp.0).unwrap(), tmp)
    }

    /// A minted key is a 128-bit hex string (32 chars) and every mint is distinct,
    /// so the capability token can't be guessed from a previous one.
    #[test]
    fn a_minted_key_is_128_bits_of_hex_and_unique() {
        let a = FsMediaStore::mint_key();
        let b = FsMediaStore::mint_key();
        assert_eq!(a.len(), 32, "128 bits = 32 hex chars");
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()), "pure hex, so path_for accepts it");
        assert_ne!(a, b, "keys are random, never a deterministic function of upload order");
    }

    /// The key is not derived from the content: two identical uploads get different
    /// keys, so no one can recompute another blob's key from its bytes.
    #[test]
    fn identical_content_gets_distinct_keys() {
        let (s, _tmp) = store();
        let k1 = s.put("image/png", b"same-bytes").unwrap();
        let k2 = s.put("image/png", b"same-bytes").unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn put_then_get_round_trips_bytes_and_content_type() {
        let (s, _tmp) = store();
        let key = s.put("image/png", b"\x89PNG\r\n").unwrap();
        let (ct, bytes) = s.get(&key).expect("the blob reads back");
        assert_eq!(ct, "image/png");
        assert_eq!(bytes, b"\x89PNG\r\n");
    }

    #[test]
    fn delete_removes_the_blob_and_its_content_type() {
        let (s, _tmp) = store();
        let key = s.put("text/plain", b"hi").unwrap();
        s.delete(&key);
        assert!(s.get(&key).is_none(), "a deleted blob is gone");
    }

    /// `path_for` is the path-traversal guard: only pure hex within the length
    /// bound is ever joined onto the media dir.
    #[test]
    fn path_for_rejects_non_hex_and_traversal() {
        let (s, _tmp) = store();
        for bad in ["", "../secret", "a/b", "..", "zzzz", "abcXYZ", &"a".repeat(65)] {
            assert!(s.path_for(bad).is_none(), "must reject `{bad}`");
        }
        assert!(s.path_for("deadbeef").is_some(), "pure hex within bound is accepted");
    }
}
