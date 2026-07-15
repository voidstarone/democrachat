//! Cross-implementation interop check: proves the WASM crypto (what the browser
//! runs) is byte-compatible with the native Rust crypto (what the CLI and server
//! tests use). Run in three steps against the node harness in `interop/`:
//!
//!   cargo run -p e2ee-wasm --example xcrypto -- emit   > /tmp/rust.json
//!   node crates/e2ee-wasm/interop/xcheck.mjs /tmp/rust.json /tmp/wasm.json
//!   cargo run -p e2ee-wasm --example xcrypto -- verify < /tmp/wasm.json
//!
//! `emit` prints vectors sealed by Rust for the browser to open; `verify` reads
//! vectors the browser sealed and confirms Rust opens them. Fixed keys are shared
//! by both sides so no key material has to cross unencrypted.

use app::{
    open_channel_message, open_sealed, seal_channel_message, seal_to, unwrap_secret, wrap_secret,
    ChannelKey, IdentitySecret,
};
use serde_json::{json, Value};

// Shared fixtures (hex). Arbitrary but fixed so both sides agree.
const SECRET_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const CHANKEY_HEX: &str = "0202020202020202020202020202020202020202020202020202020202020202";
const PASSWORD: &str = "correct horse battery staple";

fn secret() -> IdentitySecret {
    IdentitySecret::from_hex(SECRET_HEX).unwrap()
}
fn chankey() -> ChannelKey {
    ChannelKey::from_hex(CHANKEY_HEX).unwrap()
}

fn main() {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "emit" => {
            let s = secret();
            let out = json!({
                "pub": s.public().to_hex(),
                "dm": hex::encode(seal_to(&s.public(), b"dm: rust to wasm")),
                "chan": hex::encode(seal_channel_message(&chankey(), b"chan: rust to wasm")),
                "wrapped": wrap_secret(PASSWORD, &s).unwrap(),
            });
            println!("{out}");
        }
        "verify" => {
            let v: Value = serde_json::from_reader(std::io::stdin()).unwrap();
            let s = secret();

            let dm = open_sealed(&s, &hex::decode(v["dm"].as_str().unwrap()).unwrap()).unwrap();
            assert_eq!(dm, b"dm: wasm to rust", "wasm-sealed DM must open in Rust");

            let chan =
                open_channel_message(&chankey(), &hex::decode(v["chan"].as_str().unwrap()).unwrap())
                    .unwrap();
            assert_eq!(chan, b"chan: wasm to rust", "wasm-sealed channel msg must open in Rust");

            let wrapped: app::WrappedSecret = serde_json::from_value(v["wrapped"].clone()).unwrap();
            let recovered = unwrap_secret(PASSWORD, &wrapped).unwrap();
            assert_eq!(recovered.to_hex(), SECRET_HEX, "wasm-wrapped secret must unwrap in Rust");

            println!("OK: all wasm→rust vectors verified (dm, channel, password-wrap)");
        }
        other => {
            eprintln!("usage: xcrypto emit | verify   (got {other:?})");
            std::process::exit(2);
        }
    }
}
