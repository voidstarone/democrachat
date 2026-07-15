//! A user's directory entry: their public device key and password-wrapped secret.

use serde::{Deserialize, Serialize};

use crate::keys::wrapped_key::WrappedKey;
use crate::UserId;

/// A user's published device keys — the server-blind key directory entry.
///
/// `public_key` is not secret: it is handed to any sender who wants to seal a DM or
/// a channel message-key to this user. `wrapped_secret` is the user's device secret
/// wrapped under their password ([`WrappedKey`]) — the server holds it only so the
/// user can recover it on another device, and cannot open it. Homed on the user's
/// node, like the rest of the user-global surface (DMs, friends, blocks).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct UserKeys {
    pub user_id: UserId,
    /// Hex-encoded X25519 public key. Not secret.
    pub public_key: String,
    /// The user's device secret, wrapped under their password. Opaque to the server.
    pub wrapped_secret: WrappedKey,
}

impl UserKeys {
    pub fn new(user_id: UserId, public_key: impl Into<String>, wrapped_secret: WrappedKey) -> Self {
        Self {
            user_id,
            public_key: public_key.into(),
            wrapped_secret,
        }
    }
}
