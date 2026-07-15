//! Use-cases for the server-blind key directory: publish a user's device keys,
//! hand the owner back their wrapped secret, and hand any sender a public key.
//!
//! The server is **blind** to the private surface. It validates only that the
//! public key is a well-formed X25519 key (so senders can seal to it); the wrapped
//! secret is stored and returned verbatim, never opened — only the user, with their
//! password, can unwrap it client-side.

use std::sync::Arc;

use domain::{UserKeys, WrappedKey};

use crate::e2ee::public_identity::PublicIdentity;
use crate::error::key_error::KeyError;
use crate::{KeyDirectoryStore, UserStore};

/// Key-directory use-cases held on their own handle, reached via [`Services::keys`].
#[derive(Clone)]
pub struct KeyDirectoryService {
    pub(crate) keys: Arc<dyn KeyDirectoryStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

impl KeyDirectoryService {
    /// Publish (or replace) the calling user's device keys. `public_key` is the
    /// hex X25519 public key; `wrapped_secret` is the password-wrapped device
    /// secret, opaque to the server. The public key is validated so a malformed one
    /// never poisons the directory a sender relies on.
    pub fn publish_keys(
        &self,
        handle: &str,
        public_key: &str,
        wrapped_secret: WrappedKey,
    ) -> Result<(), KeyError> {
        let user = self
            .users
            .find_by_handle(handle.trim())?
            .ok_or_else(|| KeyError::NoSuchUser(handle.to_string()))?;
        // Validate shape only — the key is public, so we just ensure it parses.
        PublicIdentity::from_hex(public_key).map_err(|e| KeyError::BadPublicKey(e.to_string()))?;
        self.keys
            .put_keys(UserKeys::new(user.id, public_key.trim(), wrapped_secret))?;
        Ok(())
    }

    /// The calling user's own directory entry — including the wrapped secret, so a
    /// new device can unwrap it with the password. Serve this **only** to the
    /// authenticated owner (the caller enforces identity).
    pub fn my_keys(&self, handle: &str) -> Result<UserKeys, KeyError> {
        let user = self
            .users
            .find_by_handle(handle.trim())?
            .ok_or_else(|| KeyError::NoSuchUser(handle.to_string()))?;
        self.keys
            .get_keys(user.id)?
            .ok_or_else(|| KeyError::NotPublished(handle.to_string()))
    }

    /// Another user's **public** key, for sealing a DM or message-key to them.
    /// Never exposes the wrapped secret.
    pub fn public_key_of(&self, handle: &str) -> Result<String, KeyError> {
        let user = self
            .users
            .find_by_handle(handle.trim())?
            .ok_or_else(|| KeyError::NoSuchUser(handle.to_string()))?;
        self.keys
            .get_keys(user.id)?
            .map(|k| k.public_key)
            .ok_or_else(|| KeyError::NotPublished(handle.to_string()))
    }
}
