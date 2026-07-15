// Node half of the WASM↔Rust interop check (see examples/xcrypto.rs).
//
//   node xcheck.mjs <rust-vectors.json> <out-wasm-vectors.json>
//
// Opens the vectors Rust sealed (proving Rust→wasm), then seals fresh vectors for
// Rust to open (proving wasm→Rust) and writes them out. Uses the SAME fixed keys
// as the Rust side. Loads the wasm-bindgen module the browser uses, unchanged.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const SECRET_HEX = '0101010101010101010101010101010101010101010101010101010101010101';
const CHANKEY_HEX = '0202020202020202020202020202020202020202020202020202020202020202';
const PASSWORD = 'correct horse battery staple';

const here = dirname(fileURLToPath(import.meta.url));
const wasmDir = join(here, '..', '..', 'adapter-web', 'src', 'wasm');

// The wasm-bindgen `--target web` module expects a browser; in node we init it by
// handing it the .wasm bytes directly.
const E = await import(join(wasmDir, 'e2ee.js'));
await E.default({ module_or_path: readFileSync(join(wasmDir, 'e2ee_bg.wasm')) });

const te = new TextEncoder(), td = new TextDecoder();
const toHex = b => Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
const textToHex = s => toHex(te.encode(s));
const hexToText = h => td.decode(Uint8Array.from(h.match(/../g).map(x => parseInt(x, 16))));

const rust = JSON.parse(readFileSync(process.argv[2], 'utf8'));

// ── Rust → wasm: open what Rust sealed ──────────────────────────────────────
const pub = E.identity_public(SECRET_HEX);
if (pub !== rust.pub) throw new Error(`pubkey mismatch:\n  wasm ${pub}\n  rust ${rust.pub}`);

const dm = hexToText(E.open(SECRET_HEX, rust.dm));
if (dm !== 'dm: rust to wasm') throw new Error(`DM open mismatch: ${JSON.stringify(dm)}`);

const chan = hexToText(E.channel_open(CHANKEY_HEX, rust.chan));
if (chan !== 'chan: rust to wasm') throw new Error(`channel open mismatch: ${JSON.stringify(chan)}`);

const unwrapped = E.unwrap_secret(PASSWORD, JSON.stringify(rust.wrapped));
if (unwrapped !== SECRET_HEX) throw new Error(`unwrap mismatch:\n  ${unwrapped}\n  ${SECRET_HEX}`);

console.log('OK: all rust→wasm vectors verified (pubkey, dm, channel, password-wrap)');

// ── wasm → Rust: seal fresh vectors for Rust to open ────────────────────────
const out = {
  dm: E.seal(pub, textToHex('dm: wasm to rust')),
  chan: E.channel_seal(CHANKEY_HEX, textToHex('chan: wasm to rust')),
  wrapped: JSON.parse(E.wrap_secret(PASSWORD, SECRET_HEX)),
};
writeFileSync(process.argv[3], JSON.stringify(out));
console.log(`wrote wasm vectors → ${process.argv[3]}`);
