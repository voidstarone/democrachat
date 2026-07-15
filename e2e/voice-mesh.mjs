// End-to-end voice-mesh interop test: boots the real democrachat server, then puts
// one user in Chrome and one in Firefox into the same voice channel and asserts the
// full-mesh WebRTC peer connection actually reaches `connected` with a live remote
// audio track flowing each way. Media uses each browser's fake capture device, so no
// microphone or human is involved. Exit 0 = pass, non-zero = fail.
//
// Run directly (`node e2e/voice-mesh.mjs`) or via `cargo test --test voice_e2e`
// (which shells out here and is skipped unless DEMOCRACHAT_E2E=1).

import { spawn, execSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const PORT = 3941;
const BASE = `http://127.0.0.1:${PORT}`;
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const FIREFOX = '/Applications/Firefox.app/Contents/MacOS/firefox';

const sleep = ms => new Promise(r => setTimeout(r, ms));
function log(...a) { console.log('[e2e]', ...a); }

// ── Boot the server ────────────────────────────────────────────────────────
async function startServer() {
  const bin = join(ROOT, 'target', 'debug', 'democrachat');
  execSync(`test -x "${bin}"`); // fail loudly if the binary isn't built
  // Reject a busy port up front — a straggler from a crashed run would otherwise
  // answer /api/config and silently serve stale state (e.g. a pre-existing user).
  try { await fetch(`${BASE}/api/config`); throw new Error(`port ${PORT} already in use — kill the straggler`); }
  catch (e) { if (e.message.includes('already in use')) throw e; }
  // Isolate all server state in a throwaway temp dir (data snapshot + media),
  // set via env — the binary takes DEMOCRACHAT_DATA/DEMOCRACHAT_MEDIA_DIR, not flags,
  // so it never touches the repo-root democrachat.json.
  const dir = mkdtempSync(join(tmpdir(), 'dc-e2e-'));
  const proc = spawn(bin, ['serve', '--addr', `127.0.0.1:${PORT}`], {
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      DEMOCRACHAT_SESSION_SECRET: 'e2e-secret-not-for-prod',
      DEMOCRACHAT_DATA: join(dir, 'db.json'),
      DEMOCRACHAT_MEDIA_DIR: join(dir, 'media'),
    },
  });
  proc.stderr.on('data', d => process.env.E2E_VERBOSE && process.stderr.write(`[srv] ${d}`));
  // Wait for the port to answer.
  for (let i = 0; i < 100; i++) {
    try { const r = await fetch(`${BASE}/api/config`); if (r.ok) { log('server up'); return proc; } } catch {}
    await sleep(100);
  }
  throw new Error('server did not come up');
}

// ── In-page helpers (run inside each browser against the real app.js) ────────

// Wait until app.js has loaded and its module-scope `api`/`S` are reachable.
const waitAppReady = page => page.waitForFunction(
  () => typeof api === 'function' && typeof S === 'object', { timeout: 15000 });

// Register `handle`, then reload so the app boots authenticated.
async function register(page, handle, pw) {
  await page.goto(BASE, { waitUntil: 'domcontentloaded' });
  await waitAppReady(page);
  await page.evaluate((h, p) => api('/api/register', 'POST', { handle: h, password: p }), handle, pw);
  await page.goto(BASE, { waitUntil: 'domcontentloaded' });
  await waitAppReady(page);
  await page.waitForFunction(() => S.me != null, { timeout: 10000 });
}

// Drive the real client into a joined voice call and return once its WebSocket is up.
async function joinVoice(page, slug, channel) {
  await page.evaluate(async (s, c) => {
    await selectServer(s);
    await selectChannel(c);
  }, slug, channel);
  await page.waitForFunction(() => S.ws && S.ws.readyState === 1, { timeout: 10000 });
  // Probe getUserMedia directly so a fake-media misconfig surfaces as a clear error
  // rather than a swallowed toast inside voiceJoin's guard wrapper.
  const mic = await page.evaluate(async () => {
    try { const s = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      const n = s.getAudioTracks().length; s.getTracks().forEach(t => t.stop()); return { ok: true, tracks: n }; }
    catch (e) { return { ok: false, err: String(e && e.name || e) }; }
  });
  if (!mic.ok) throw new Error(`getUserMedia failed on ${slug}/${channel}: ${mic.err}`);
  await page.evaluate((s, c) => voiceJoin(s, c), slug, channel);
  await page.waitForFunction(() => S.voice != null, { timeout: 8000 });
}

// Poll until this page has a peer whose connection is established with a remote
// audio track, or time out. Returns a diagnostic snapshot either way.
function meshEstablished(page) {
  return page.waitForFunction(() => {
    const v = S.voice; if (!v) return false;
    const peers = Object.values(v.peers);
    if (!peers.length) return false;
    return peers.some(p => {
      const pc = p.pc; if (!pc) return false;
      const up = pc.connectionState === 'connected'
        || pc.iceConnectionState === 'connected' || pc.iceConnectionState === 'completed';
      const track = p.audio && p.audio.srcObject
        && p.audio.srcObject.getAudioTracks && p.audio.srcObject.getAudioTracks().length > 0;
      return up && track;
    });
  }, { timeout: 25000 }).then(() => true);
}

function snapshot(page) {
  return page.evaluate(() => {
    const v = S.voice; if (!v) return { joined: false };
    return {
      joined: true, myId: v.myId,
      peers: Object.entries(v.peers).map(([id, p]) => ({
        id, handle: p.handle,
        connectionState: p.pc && p.pc.connectionState,
        iceConnectionState: p.pc && p.pc.iceConnectionState,
        remoteAudioTracks: (p.audio && p.audio.srcObject && p.audio.srcObject.getAudioTracks
          && p.audio.srcObject.getAudioTracks().length) || 0,
      })),
    };
  });
}

// ── Main ────────────────────────────────────────────────────────────────────
let server, chrome, firefox;
try {
  server = await startServer();

  log('launching Chrome + Firefox (fake media)…');
  chrome = await puppeteer.launch({
    browser: 'chrome', executablePath: CHROME, headless: true,
    args: [
      '--no-sandbox',
      '--use-fake-device-for-media-stream',
      '--use-fake-ui-for-media-stream',
      '--autoplay-policy=no-user-gesture-required',
    ],
  });
  firefox = await puppeteer.launch({
    browser: 'firefox', executablePath: FIREFOX, headless: true,
    extraPrefsFirefox: {
      'media.navigator.streams.fake': true,
      'media.navigator.permission.disabled': true,
      'media.autoplay.default': 0,
      'permissions.default.microphone': 1,
    },
  });

  const alice = await chrome.newPage();
  const bob = await firefox.newPage();
  globalThis.__alice = alice; globalThis.__bob = bob;
  for (const [name, pg] of [['chrome/alice', alice], ['firefox/bob', bob]]) {
    pg.on('pageerror', e => log(`${name} pageerror:`, e.message));
    if (process.env.E2E_VERBOSE) pg.on('console', m => log(`${name} console:`, m.text()));
  }

  // Alice (Chrome) registers, founds a server, adds a voice channel.
  await register(alice, 'alice', 'correct horse battery staple');
  const slug = await alice.evaluate(async () => {
    const s = await api('/api/servers', 'POST', { name: 'Voice Test' });
    await api(`/api/servers/${s.slug}/channels`, 'POST', { name: 'lounge', topic: '', kind: 'voice' });
    return s.slug;
  });
  log('server founded:', slug);

  // Bob (Firefox) registers and joins that server.
  await register(bob, 'bob', 'correct horse battery staple');
  await bob.evaluate(s => api(`/api/servers/${s}/join`, 'POST', {}), slug);
  log('bob joined');

  // Both walk into the voice channel and join the call.
  await joinVoice(alice, slug, 'lounge');
  await sleep(400);
  await joinVoice(bob, slug, 'lounge');
  log('both joined voice — waiting for mesh…');

  await Promise.all([meshEstablished(alice), meshEstablished(bob)]);

  const [sa, sb] = await Promise.all([snapshot(alice), snapshot(bob)]);
  log('chrome/alice:', JSON.stringify(sa));
  log('firefox/bob :', JSON.stringify(sb));
  log('PASS: Chrome↔Firefox voice mesh connected with remote audio both ways.');
  process.exitCode = 0;
} catch (err) {
  console.error('[e2e] FAIL:', err && err.message ? err.message : err);
  for (const [name, pg] of [['chrome/alice', globalThis.__alice], ['firefox/bob', globalThis.__bob]]) {
    try { if (pg) log(`${name} state:`, JSON.stringify(await snapshot(pg))); } catch {}
  }
  process.exitCode = 1;
} finally {
  try { await chrome?.close(); } catch {}
  try { await firefox?.close(); } catch {}
  try { server?.kill('SIGKILL'); } catch {}
}
