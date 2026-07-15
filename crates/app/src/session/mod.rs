//! Session-cookie signing (HMAC-SHA256) and constant-time comparison. The signer
//! is key-injected by the composition root; the web adapter turns a verified
//! token into the request's authenticated identity.
pub mod constant_time_eq;
pub mod session_signer;
