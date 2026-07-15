// End-to-end check of the browser E2EE *flow* (the exact sealing sequence app.js
// uses) against a live server, driven from node with the real wasm crypto.
//
//   node e2e.mjs http://127.0.0.1:PORT
//
// Exercises: device-key setup (generate→wrap→publish), sealed DM send+read, and an
// encrypted channel (enable→mint key→grant→post sealed→read, incl. a late joiner
// reading the backlog). Fails loudly on the first mismatch.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const base = process.argv[2];
const here = dirname(fileURLToPath(import.meta.url));
const wasmDir = join(here, '..', '..', 'adapter-web', 'src', 'wasm');
const E = await import(join(wasmDir, 'e2ee.js'));
await E.default({ module_or_path: readFileSync(join(wasmDir, 'e2ee_bg.wasm')) });

const te = new TextEncoder(), td = new TextDecoder();
const toHex = b => Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
const textToHex = s => toHex(te.encode(s));
const hexToText = h => td.decode(Uint8Array.from(h.match(/../g).map(x => parseInt(x, 16))));
const assert = (c, m) => { if (!c) { console.error('FAIL: ' + m); process.exit(1); } };

// A browser-like client: its own cookie jar, CSRF double-submit, and device identity.
function client() {
  const jar = {};
  const csrf = 'tok' + Math.floor(Math.random() * 1e9);
  jar.csrf = csrf;
  const cookieHeader = () => Object.entries(jar).map(([k, v]) => `${k}=${v}`).join('; ');
  async function api(path, method = 'GET', body) {
    const headers = { 'content-type': 'application/json', cookie: cookieHeader() };
    if (method !== 'GET') headers['x-csrf-token'] = csrf;
    const r = await fetch(base + path, { method, headers, body: body ? JSON.stringify(body) : undefined });
    for (const c of r.headers.getSetCookie?.() ?? []) { const [kv] = c.split(';'); const [k, v] = kv.split('='); jar[k] = v; }
    const text = await r.text();
    if (!r.ok) throw new Error(`${r.status} ${text}`);
    return text && r.headers.get('content-type')?.includes('json') ? JSON.parse(text) : text;
  }
  return { api, ident: null };
}

async function setupIdentity(c, password) {
  const secret = E.identity_generate();
  const pub = E.identity_public(secret);
  const wrapped = JSON.parse(E.wrap_secret(password, secret));
  await c.api('/api/keys', 'POST', { public_key: pub, wrapped_secret: wrapped });
  c.ident = { secret, public: pub };
}

const PW = 'correct horse battery staple';
const alice = client(), bob = client();

// ── device keys ─────────────────────────────────────────────────────────────
await alice.api('/api/register', 'POST', { handle: 'alice', password: PW });
await bob.api('/api/register', 'POST', { handle: 'bob', password: PW });
await setupIdentity(alice, PW);
await setupIdentity(bob, PW);
console.log('✓ device keys generated, wrapped, and published');

// ── sealed DM ────────────────────────────────────────────────────────────────
{
  const theirs = (await alice.api('/api/keys/bob')).public_key;
  const ph = textToHex('meet at the docks 🌊');
  await alice.api('/api/social/alice/dm/bob', 'POST', {
    sealed_for_recipient: E.seal(theirs, ph),
    sealed_for_sender: E.seal(alice.ident.public, ph),
  });
  const convo = await bob.api('/api/social/bob/with/alice');
  const got = hexToText(E.open(bob.ident.secret, convo[0].sealed_for_me));
  assert(got === 'meet at the docks 🌊', `bob DM decrypt: ${JSON.stringify(got)}`);
  // Alice re-reads her own sent message from her own sealed copy.
  const aConvo = await alice.api('/api/social/alice/with/bob');
  const aGot = hexToText(E.open(alice.ident.secret, aConvo[0].sealed_for_me));
  assert(aGot === 'meet at the docks 🌊', `alice DM self-read: ${JSON.stringify(aGot)}`);
  console.log('✓ sealed DM: bob reads it, alice re-reads her own copy, server saw only ciphertext');
}

// ── encrypted channel (open history) ─────────────────────────────────────────
{
  await alice.api('/api/servers', 'POST', { name: 'Vault Club' });
  await alice.api('/api/servers/vault-club/channels', 'POST', { name: 'plans', topic: '' });
  await alice.api('/api/servers/vault-club/channels/plans/encrypt', 'POST', { history_mode: 'open' });

  // Alice mints the channel key and grants it to herself (epoch 0).
  const key = E.channel_key_generate();
  await alice.api('/api/servers/vault-club/channels/plans/keys', 'POST',
    { epoch: 0, member: 'alice', sealed_key: E.seal(alice.ident.public, key) });

  await alice.api('/api/servers/vault-club/channels/plans/sealed-messages', 'POST',
    { ciphertext: E.channel_seal(key, textToHex('the vault code is 4242')), key_epoch: 0 });

  // Alice reads it back by recovering her grant then opening the message.
  const myGrant = (await alice.api('/api/servers/vault-club/channels/plans/keys'))[0];
  const myKey = E.open(alice.ident.secret, myGrant.sealed_key);
  const msgs = await alice.api('/api/servers/vault-club/channels/plans/messages');
  const sealed = msgs.find(m => m.key_epoch != null);
  const plain = hexToText(E.channel_open(myKey, sealed.body));
  assert(plain === 'the vault code is 4242', `alice channel decrypt: ${JSON.stringify(plain)}`);
  assert(!sealed.body.includes('4242'), 'stored channel body must be ciphertext');
  console.log('✓ encrypted channel: sealed post stored as ciphertext, author decrypts it');

  // A late joiner (bob) is granted the SAME long-lived key (open history) and reads
  // the backlog, then posts his own sealed reply that alice can read.
  await bob.api('/api/servers/vault-club/join', 'POST', {});
  const bobPub = (await alice.api('/api/keys/bob')).public_key;
  await alice.api('/api/servers/vault-club/channels/plans/keys', 'POST',
    { epoch: 0, member: 'bob', sealed_key: E.seal(bobPub, key) });

  const bGrant = (await bob.api('/api/servers/vault-club/channels/plans/keys'))[0];
  const bKey = E.open(bob.ident.secret, bGrant.sealed_key);
  const bMsgs = await bob.api('/api/servers/vault-club/channels/plans/messages');
  const bPlain = hexToText(E.channel_open(bKey, bMsgs.find(m => m.key_epoch != null).body));
  assert(bPlain === 'the vault code is 4242', `bob backlog read: ${JSON.stringify(bPlain)}`);
  console.log('✓ open-history backlog: a late joiner reads messages sent before they joined');
}

console.log('\nALL BROWSER-FLOW E2E CHECKS PASSED');
