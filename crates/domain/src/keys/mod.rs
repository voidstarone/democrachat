//! The server-blind key directory: a user's published device keys.
//!
//! A user's device keypair is X25519. The **public** half is stored in the clear
//! (anyone may seal a DM or a message-key to it); the **secret** half is held only
//! wrapped under the user's password ([`WrappedKey`]) — an opaque blob the server
//! stores verbatim and can never open. The server is blind to the private surface;
//! it only routes ciphertext and hands out public keys.

pub mod user_keys;
pub mod wrapped_key;
