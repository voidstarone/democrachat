# e2ee-wasm

The E2EE crypto ([`app::e2ee`]) compiled to WebAssembly, so the **browser** runs
the exact same Rust code as the CLI and the server-side tests. There is only one
implementation of the sealing/opening math, so the wire formats can never drift.

Everything crosses the JS boundary as **hex strings**; message text is UTF-8. The
browser holds the device secret (and channel keys); the server never does.

## Regenerating the browser artifacts

The generated glue + wasm are committed under `../adapter-web/src/wasm/` and baked
into the server binary (`include_str!` / `include_bytes!`). **Whenever the crypto in
`app::e2ee` changes, regenerate them:**

```sh
# from the workspace root
cargo build -p e2ee-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript \
  --out-dir crates/adapter-web/src/wasm --out-name e2ee \
  target/wasm32-unknown-unknown/release/e2ee_wasm.wasm
```

Prereqs: `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli` (the CLI version must match the `wasm-bindgen`
pin in `Cargo.toml`).

## Verifying wire-compatibility (native ↔ wasm)

`interop/` cross-checks that the wasm crypto is byte-compatible with native Rust,
in both directions (sealed box, channel messages, and the Argon2id password-wrap):

```sh
cargo run -p e2ee-wasm --example xcrypto -- emit > /tmp/rust.json
node crates/e2ee-wasm/interop/xcheck.mjs /tmp/rust.json /tmp/wasm.json
cargo run -p e2ee-wasm --example xcrypto -- verify < /tmp/wasm.json
```

And `interop/e2e.mjs` drives the full browser E2EE flow (device-key setup, sealed
DMs, encrypted channels with an open-history backlog) against a running server:

```sh
node crates/e2ee-wasm/interop/e2e.mjs http://127.0.0.1:3737
```
