//! The E2EE primitives, compiled to WebAssembly so the **browser** runs the exact
//! same Rust crypto as the CLI and the server-side tests — the wire formats can
//! never drift, because there is only one implementation ([`app::e2ee`]).
//!
//! Everything crosses the JS boundary as **hex strings** (binary is hex-encoded),
//! matching how the server already stores these blobs, so the JS glue is trivial:
//! `TextEncoder`/`TextDecoder` for message text, hex for keys and ciphertext.
//!
//! Nothing here is a use-case or a policy — it is only the sealing/opening math.
//! The browser holds the device secret and channel keys; the server never does.

use wasm_bindgen::prelude::*;

/// Map any error to a JS exception carrying its message.
fn js_err(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}

// ── Device identity (X25519) ────────────────────────────────────────────────

/// Mint a fresh device secret, returned as 64-char hex for the client to persist
/// locally (IndexedDB) and to wrap under the password.
#[wasm_bindgen]
pub fn identity_generate() -> String {
    app::IdentitySecret::generate().to_hex()
}

/// The public key (hex) for a given secret (hex) — what a sender seals to.
#[wasm_bindgen]
pub fn identity_public(secret_hex: &str) -> Result<String, JsValue> {
    Ok(app::IdentitySecret::from_hex(secret_hex).map_err(js_err)?.public().to_hex())
}

// ── Password wrap (Argon2id) ────────────────────────────────────────────────

/// Wrap a device secret under the password, returning the `{salt,nonce,ciphertext}`
/// JSON the key directory stores. The server is blind to it.
#[wasm_bindgen]
pub fn wrap_secret(password: &str, secret_hex: &str) -> Result<String, JsValue> {
    let secret = app::IdentitySecret::from_hex(secret_hex).map_err(js_err)?;
    let wrapped = app::wrap_secret(password, &secret).map_err(js_err)?;
    serde_json::to_string(&wrapped).map_err(js_err)
}

/// Recover a device secret (hex) from the wrapped `{salt,nonce,ciphertext}` JSON and
/// the password. Errors on the wrong password.
#[wasm_bindgen]
pub fn unwrap_secret(password: &str, wrapped_json: &str) -> Result<String, JsValue> {
    let wrapped: app::WrappedSecret = serde_json::from_str(wrapped_json).map_err(js_err)?;
    Ok(app::unwrap_secret(password, &wrapped).map_err(js_err)?.to_hex())
}

// ── Sealed box (DMs, and sealing a channel key into a grant) ─────────────────

/// Seal `plaintext_hex` to a recipient's public key (hex); returns the sealed blob
/// as hex. Used for DM bodies and for sealing a channel key into a member's grant.
#[wasm_bindgen]
pub fn seal(recipient_pub_hex: &str, plaintext_hex: &str) -> Result<String, JsValue> {
    let pk = app::PublicIdentity::from_hex(recipient_pub_hex).map_err(js_err)?;
    let plaintext = hex::decode(plaintext_hex).map_err(js_err)?;
    Ok(hex::encode(app::seal_to(&pk, &plaintext)))
}

/// Open a sealed blob (hex) with the holder's device secret (hex); returns the
/// plaintext as hex. Errors if it wasn't sealed to this key.
#[wasm_bindgen]
pub fn open(secret_hex: &str, sealed_hex: &str) -> Result<String, JsValue> {
    let secret = app::IdentitySecret::from_hex(secret_hex).map_err(js_err)?;
    let sealed = hex::decode(sealed_hex).map_err(js_err)?;
    Ok(hex::encode(app::open_sealed(&secret, &sealed).map_err(js_err)?))
}

// ── Channel key + sealed channel messages ───────────────────────────────────

/// Mint a fresh symmetric channel key, as 32-byte hex.
#[wasm_bindgen]
pub fn channel_key_generate() -> String {
    app::ChannelKey::generate().to_hex()
}

/// Seal `plaintext_hex` under a channel key (hex); returns the sealed body as hex.
#[wasm_bindgen]
pub fn channel_seal(key_hex: &str, plaintext_hex: &str) -> Result<String, JsValue> {
    let key = app::ChannelKey::from_hex(key_hex).map_err(js_err)?;
    let plaintext = hex::decode(plaintext_hex).map_err(js_err)?;
    Ok(hex::encode(app::seal_channel_message(&key, &plaintext)))
}

/// Open a sealed channel body (hex) with the channel key (hex); returns plaintext
/// as hex. Errors on the wrong key or a tampered body.
#[wasm_bindgen]
pub fn channel_open(key_hex: &str, sealed_hex: &str) -> Result<String, JsValue> {
    let key = app::ChannelKey::from_hex(key_hex).map_err(js_err)?;
    let sealed = hex::decode(sealed_hex).map_err(js_err)?;
    Ok(hex::encode(app::open_channel_message(&key, &sealed).map_err(js_err)?))
}
