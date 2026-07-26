const S = { me:null, mode:'chat',
            server:null, channel:null, reply:null, isDev:false, tier:null, serverDetail:null,
            peer:null, social:null, friendTab:'all', mentionable:{users:[],roles:[]}, emojiMap:{},
            ident:null, channels:[], chanKeys:{} };
const $ = id => document.getElementById(id);

/* ── Internationalisation ─────────────────────────────────────────────
   Message catalogs are injected as `window.__CATALOGS__` ahead of this script
   (see the `/app.js` route), so `t()` resolves synchronously at first paint.
   Keys are dot-namespaced; values may hold `{param}` placeholders. Lookup falls
   back active-locale → English → the key itself, so a missing translation
   degrades gracefully rather than blanking the UI. */
const CATALOGS = (typeof window !== 'undefined' && window.__CATALOGS__) || { en: {} };
const LANGS = [{code:'en',label:'English'},{code:'es',label:'Español'}];
function detectLang(){
  const saved = localStorage.getItem('lang');
  if (saved && CATALOGS[saved]) return saved;
  const nav = (navigator.language || 'en').slice(0,2).toLowerCase();
  return CATALOGS[nav] ? nav : 'en';
}
let LANG = detectLang();
function t(key, params){
  const active = CATALOGS[LANG] || {}, en = CATALOGS.en || {};
  let s = key in active ? active[key] : (key in en ? en[key] : key);
  if (params) for (const k in params) s = s.split('{'+k+'}').join(params[k]);
  return s;
}
/* Localise the static shell: elements carry `data-t` (textContent),
   `data-t-html` (innerHTML), `data-t-ph` (placeholder), or `data-t-title`
   (title/tooltip) naming a catalog key. Re-run whenever the language changes. */
function applyLang(){
  document.documentElement.setAttribute('lang', LANG);
  document.querySelectorAll('[data-t]').forEach(el=> el.textContent = t(el.dataset.t));
  document.querySelectorAll('[data-t-html]').forEach(el=> el.innerHTML = t(el.dataset.tHtml));
  document.querySelectorAll('[data-t-ph]').forEach(el=> el.setAttribute('placeholder', t(el.dataset.tPh)));
  document.querySelectorAll('[data-t-title]').forEach(el=> el.setAttribute('title', t(el.dataset.tTitle)));
}
function setLang(l){ if(!CATALOGS[l]) return; LANG=l; localStorage.setItem('lang', l); applyLang(); }

/* CSRF double-submit: a client-generated token kept in a readable `dc_csrf` cookie
   and echoed in the X-DC-CSRF-Token header on every mutation. The server checks the
   two match. A cross-site page can neither read this cookie nor set the header.
   The name is app-namespaced so a sibling app sharing this localhost origin (which
   may have set a plain, possibly-HttpOnly `csrf` cookie our JS can't touch) can't
   shadow ours and break the double-submit. */
function csrfToken(){
  const found = document.cookie.split(';').map(s=>s.trim()).find(s=>s.startsWith('dc_csrf='));
  const val = found ? found.slice(8) : '';
  if (val) return val;                     // a blank `dc_csrf=` must be regenerated, not reused
  const bytes = new Uint8Array(16); crypto.getRandomValues(bytes);
  const tok = Array.from(bytes, b=>b.toString(16).padStart(2,'0')).join('');
  document.cookie = `dc_csrf=${tok}; Path=/; SameSite=Lax${location.protocol==='https:'?'; Secure':''}`;
  return tok;
}
// Establish the CSRF cookie eagerly at load, so the first mutation (login) reads an
// already-committed cookie instead of setting and sending it in the same tick.
csrfToken();
const api = async (path, method, body) => {
  const m = method || 'GET';
  const headers = {'Content-Type':'application/json'};
  if (m !== 'GET' && m !== 'HEAD') headers['X-DC-CSRF-Token'] = csrfToken();
  const r = await fetch(path, { method: m, headers, body: body ? JSON.stringify(body) : undefined });
  // Server errors arrive as a catalog key (e.g. `err.no_such_server`); localise it.
  // Unknown text passes through `t()` untouched, so legacy prose still shows.
  if (!r.ok) throw new Error(t((await r.text()).trim()));
  return r.headers.get('content-type')?.includes('json') ? r.json() : r.text();
};
const enc = encodeURIComponent;

/* ── End-to-end encryption (WebAssembly) ─────────────────────────────
   The browser runs the exact same Rust crypto as the CLI and the server-side
   tests, compiled to wasm — so the wire formats can never drift. Binary crosses
   the boundary as hex; message text is UTF-8. The device secret lives only on this
   device (localStorage) and, wrapped under the password, in the key directory. */
let E = null; // the wasm module, loaded on first use
async function crypto_(){ if(E) return E; const m = await import('/wasm/e2ee.js'); await m.default(); E = m; return E; }
const _te = new TextEncoder(), _td = new TextDecoder();
function hexEnc(bytes){ return Array.from(bytes, b=>b.toString(16).padStart(2,'0')).join(''); }
function hexDec(hex){ const a=new Uint8Array(hex.length/2); for(let i=0;i<a.length;i++) a[i]=parseInt(hex.substr(i*2,2),16); return a; }
function textToHex(s){ return hexEnc(_te.encode(s)); }
function hexToText(h){ return _td.decode(hexDec(h)); }
function identStoreKey(){ return 'dc_ident_'+S.me; }

/* Make sure this device has the user's key identity in memory (`S.ident`). If a
   secret is stored locally, use it. Otherwise, with the password we can either
   recover it from the server's wrapped blob (a new device) or, first time, mint one
   and publish the public key + password-wrapped secret. Without a password (a
   cookie-restored session on a device with no local key) the private surface stays
   locked until the user signs in again. */
async function ensureIdentity(password){
  if(!S.me) { S.ident=null; return null; }
  await crypto_();
  const stored = localStorage.getItem(identStoreKey());
  if(stored){ S.ident = { secret: stored, public: E.identity_public(stored) }; return S.ident; }
  let mine = null; try { mine = await api('/api/keys/me'); } catch {}
  if(mine && mine.wrapped_secret){
    if(!password){ S.ident=null; return null; } // can't unwrap without the password
    try {
      const secret = E.unwrap_secret(password, JSON.stringify(mine.wrapped_secret));
      localStorage.setItem(identStoreKey(), secret);
      S.ident = { secret, public: E.identity_public(secret) };
      return S.ident;
    } catch { S.ident=null; toast(t('app.toast.unlock_failed'),'err'); return null; }
  }
  if(!password){ S.ident=null; return null; } // first-time setup needs the password to wrap
  const secret = E.identity_generate(), pub = E.identity_public(secret);
  const wrapped = JSON.parse(E.wrap_secret(password, secret));
  await api('/api/keys','POST',{ public_key: pub, wrapped_secret: wrapped });
  localStorage.setItem(identStoreKey(), secret);
  S.ident = { secret, public: pub };
  return S.ident;
}

/* ── Toasts ─────────────────────────────────────────────────────── */
function toast(msg, kind){
  const t = document.createElement('div'); t.className = 'toast '+(kind||''); t.textContent = msg;
  $('toasts').appendChild(t);
  setTimeout(()=>{ t.style.transition='opacity .3s'; t.style.opacity='0'; setTimeout(()=>t.remove(),300); }, kind==='err'?4200:2600);
}
/* Wrap an async action so any thrown error surfaces as a toast, not a crash. */
const guard = fn => (...a) => Promise.resolve().then(()=>fn(...a)).catch(e=>toast(e.message||String(e),'err'));

/* Read a File as base64 (without the `data:…;base64,` prefix — the server adds
   the right one after validating the image). */
function fileToBase64(file){
  return new Promise((res,rej)=>{
    const r = new FileReader();
    r.onload = () => { const s = String(r.result); const c = s.indexOf(','); res(c>=0 ? s.slice(c+1) : s); };
    r.onerror = () => rej(r.error);
    r.readAsDataURL(file);
  });
}

/* ── Modal (promise-based, replaces prompt) ─────────────────────── */
/* fields: [{name,label,type:'text|textarea|select|emoji',placeholder,options,value}]
   Resolves to a values object, or null if cancelled. */
function modal({title, message, fields=[], submitText=t('app.btn.ok')}){
  return new Promise(resolve=>{
    const scrim = document.createElement('div'); scrim.id='scrim';
    const emojiField = fields.find(f=>f.type==='emoji');
    const body = fields.map(f=>{
      if(f.type==='emoji'){
        const set = ['👍','❤️','😂','🎉','🔥','🙏','👀','✅','🚀','🤔'];
        return `<div class="emojibar">${set.map(e=>`<button type="button" data-emoji="${e}">${e}</button>`).join('')}</div>`;
      }
      const inner = f.type==='textarea'
        ? `<textarea name="${f.name}" placeholder="${f.placeholder||''}">${f.value||''}</textarea>`
        : f.type==='select'
        ? `<select name="${f.name}">${f.options.map(o=>`<option value="${o.value}" ${o.value===f.value?'selected':''}>${o.label}</option>`).join('')}</select>`
        : f.type==='file'
        ? `<input name="${f.name}" type="file" accept="${f.accept||''}" />`
        : `<input name="${f.name}" type="text" placeholder="${f.placeholder||''}" value="${f.value||''}" />`;
      return `<label class="field"><span>${f.label}</span>${inner}</label>`;
    }).join('');
    scrim.innerHTML = `<form class="modal">
        <h3>${esc(title)}</h3>
        ${message?`<span class="msg">${esc(message)}</span>`:''}
        ${body}
        <div class="actions">
          <button type="button" class="ghost" data-cancel>${t('app.btn.cancel')}</button>
          ${emojiField?'':`<button type="submit">${esc(submitText)}</button>`}
        </div>
      </form>`;
    document.body.appendChild(scrim);
    requestAnimationFrame(()=>scrim.classList.add('show'));
    const form = scrim.querySelector('form');
    const close = val => { scrim.classList.remove('show'); setTimeout(()=>scrim.remove(),160); resolve(val); };
    scrim.querySelector('[data-cancel]').onclick = ()=>close(null);
    scrim.onclick = e => { if(e.target===scrim) close(null); };
    if(emojiField){ scrim.querySelectorAll('[data-emoji]').forEach(b=> b.onclick=()=>close({[emojiField.name]:b.dataset.emoji})); }
    form.onsubmit = async e => { e.preventDefault();
      const vals={};
      for(const [k,v] of new FormData(form).entries()){
        if(v instanceof File){ vals[k] = v.size ? await fileToBase64(v) : ''; }
        else { vals[k] = v.trim(); }
      }
      close(vals); };
    const first = form.querySelector('input,textarea,select'); if(first) first.focus();
  });
}

/* ── Avatars ────────────────────────────────────────────────────── */
function avatar(handle, cls){
  const colors = ['#0a7d5a','#3b6ea5','#a5603b','#7a3ba5','#a53b5e','#3ba58f','#8a8f3b','#5e3ba5'];
  let h=0; for(const c of handle) h=(h*31+c.charCodeAt(0))>>>0;
  const bg = colors[h%colors.length];
  return `<div class="ava ${cls||''}" style="background:${bg}">${esc((handle[0]||'?').toUpperCase())}</div>`;
}

/* Inline SVG icons (stroke = currentColor, so they track the theme). No emoji —
   there is no icon font on the page (the CSP forbids external assets), so the UI
   draws its glyphs as self-contained SVG paths. */
const ICONS = {
  people:  '<path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
  message: '<path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>',
  check:   '<polyline points="20 6 9 17 4 12"/>',
  ban:     '<circle cx="12" cy="12" r="10"/><line x1="4.9" y1="4.9" x2="19.1" y2="19.1"/>',
  lock:    '<rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  mail:    '<path d="M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"/><polyline points="22,6 12,13 2,6"/>',
  sun:     '<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.2" y1="4.2" x2="5.6" y2="5.6"/><line x1="18.4" y1="18.4" x2="19.8" y2="19.8"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.2" y1="19.8" x2="5.6" y2="18.4"/><line x1="18.4" y1="5.6" x2="19.8" y2="4.2"/>',
  moon:    '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>',
  vote:    '<path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/>',
  gear:    '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  reply:   '<polyline points="9 14 4 9 9 4"/><path d="M20 20v-7a4 4 0 0 0-4-4H4"/>',
  attach:  '<path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48"/>',
  film:    '<rect x="2" y="3" width="20" height="18" rx="2" ry="2"/><line x1="7" y1="3" x2="7" y2="21"/><line x1="17" y1="3" x2="17" y2="21"/><line x1="2" y1="9" x2="22" y2="9"/><line x1="2" y1="15" x2="22" y2="15"/>',
  music:   '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
};
function icon(name, sz){
  const s = sz||18;
  return `<svg class="i" viewBox="0 0 24 24" width="${s}" height="${s}" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]||''}</svg>`;
}

/* ── Theme ──────────────────────────────────────────────────────── */
function themeNow(){ return document.documentElement.getAttribute('data-theme')
  || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark':'light'); }
function applyThemeButton(){ $('themeBtn').innerHTML = icon(themeNow()==='dark' ? 'sun' : 'moon', 17); }
function toggleTheme(){ const next = themeNow()==='dark'?'light':'dark';
  document.documentElement.setAttribute('data-theme', next); localStorage.setItem('theme', next); applyThemeButton(); }
(function initTheme(){ const saved = localStorage.getItem('theme');
  if (saved) document.documentElement.setAttribute('data-theme', saved); })();

/* ── Per-user display preferences (saved on this device) ─────────── */
const PREFS = { avatars:true, timestamps:true, compact:false };
function applyPrefs(){
  document.body.classList.toggle('no-avatars', !PREFS.avatars);
  document.body.classList.toggle('no-timestamps', !PREFS.timestamps);
  document.body.classList.toggle('compact', !!PREFS.compact);
}
function loadPrefs(){
  try { Object.assign(PREFS, JSON.parse(localStorage.getItem('prefs')||'{}')); } catch {}
  applyPrefs();
}
function setPref(k,v){ PREFS[k]=v; localStorage.setItem('prefs', JSON.stringify(PREFS)); applyPrefs(); }
function openSettings(){
  const scrim=document.createElement('div'); scrim.id='scrim';
  scrim.innerHTML = `<div class="modal">
    <h3>${t('app.settings.title')}</h3>
    <span class="msg">${t('app.settings.saved_note')}</span>
    <label class="switch full" style="justify-content:space-between;margin-bottom:.7rem">
      <span>${t('app.settings.show_avatars')}</span>
      <span style="display:flex"><input type="checkbox" id="prefAva" ${PREFS.avatars?'checked':''}><span class="track"></span></span>
    </label>
    <label class="switch full" style="justify-content:space-between;margin-bottom:.7rem">
      <span>${t('app.settings.show_timestamps')}</span>
      <span style="display:flex"><input type="checkbox" id="prefTs" ${PREFS.timestamps?'checked':''}><span class="track"></span></span>
    </label>
    <label class="switch full" style="justify-content:space-between;margin-bottom:.7rem">
      <span>${t('app.settings.compact')}<br><span class="sub">${t('app.settings.compact_sub')}</span></span>
      <span style="display:flex"><input type="checkbox" id="prefCompact" ${PREFS.compact?'checked':''}><span class="track"></span></span>
    </label>
    <label class="field"><span>${t('app.settings.language')}</span>
      <select id="prefLang">${LANGS.map(l=>`<option value="${l.code}" ${l.code===LANG?'selected':''}>${esc(l.label)}</option>`).join('')}</select>
    </label>
    <div class="actions"><button data-close>${t('app.btn.done')}</button></div>
  </div>`;
  document.body.appendChild(scrim);
  requestAnimationFrame(()=>scrim.classList.add('show'));
  const close=()=>{ scrim.classList.remove('show'); setTimeout(()=>scrim.remove(),160); };
  scrim.querySelector('[data-close]').onclick=close;
  scrim.onclick=e=>{ if(e.target===scrim) close(); };
  $('prefAva').onchange=e=>setPref('avatars',e.target.checked);
  $('prefTs').onchange=e=>setPref('timestamps',e.target.checked);
  $('prefCompact').onchange=e=>setPref('compact',e.target.checked);
  $('prefLang').onchange=e=>{ const val=e.target.value; setLang(val); if(S.server) selectServer(S.server); if(S.me) loadSocial(); };
}
loadPrefs();

/* ── Boot / auth ────────────────────────────────────────────────── */
/* Identity is established by the HttpOnly `sid` session cookie — never by a
   stored handle. On boot we ask the server who we are (from that cookie). */
let AUTH_MODE = 'login'; // or 'register'
async function boot() {
  applyLang();
  applyThemeButton();
  // Surface the outcome of an email-verification link, then strip the flag from the
  // URL so a refresh doesn't repeat the toast.
  const params = new URLSearchParams(location.search);
  if (params.has('verified') || params.has('verify_error')) {
    toast(params.has('verified') ? t('app.toast.email_verified') : t('app.toast.verify_failed'),
          params.has('verified') ? '' : 'err');
    history.replaceState(null, '', location.pathname);
  }
  $('settingsBtn').innerHTML = icon('gear', 17);
  const cfg = await api('/api/config'); S.isDev = cfg.is_dev;
  if (S.isDev) { $('devBox').style.display='block'; $('clockNow').textContent = 'now: '+new Date(cfg.now*1000).toISOString().slice(0,10); }
  if (cfg.me) {
    S.me = cfg.me;
    // Session restored from a cookie — no password here, so we can only use a key
    // already stored on this device; otherwise the private surface stays locked.
    try { await ensureIdentity(null); } catch {}
    afterLogin();
  }
}
function toggleAuthMode(){
  AUTH_MODE = AUTH_MODE==='login' ? 'register' : 'login';
  const reg = AUTH_MODE==='register';
  $('authBtnLabel').textContent = reg ? t('app.auth.create_account') : t('app.auth.sign_in');
  $('authSub').textContent = reg ? t('app.auth.sub_register') : t('app.auth.sub_login');
  $('authSwitchText').textContent = reg ? t('app.auth.have_account') : t('app.auth.new_here');
  $('authSwitch').textContent = reg ? t('app.auth.sign_in') : t('app.auth.create_account_link');
  $('emailInput').style.display = reg ? '' : 'none';
  $('passwordInput').setAttribute('autocomplete', reg ? 'new-password' : 'current-password');
  $('verifyNotice').style.display = 'none';
}
// Finish a successful auth: recover/set up this device's encryption keys (while we
// still hold the password — it never leaves the browser) and enter the app.
async function completeLogin(handle, password){
  S.me = handle;
  try { await ensureIdentity(password); } catch(e){ toast(t('app.toast.enc_setup_failed',{err:e.message||e}),'err'); }
  $('passwordInput').value=''; afterLogin();
}
const submitAuth = guard(async () => {
  const handle = $('handleInput').value.trim();
  const password = $('passwordInput').value;
  if (!handle || !password) { toast(t('app.toast.enter_handle_pw'),'err'); return; }
  if (AUTH_MODE==='register') {
    const email = $('emailInput').value.trim();
    if (!email) { toast(t('app.toast.enter_email'),'err'); return; }
    const r = await api('/api/register','POST',{handle, password, email});
    if (r && r.verify_required) {
      // Hard mode: account created but not logged in until the emailed link is clicked.
      S.pendingVerifyHandle = handle;
      $('passwordInput').value='';
      $('verifyNotice').style.display='block';
      toast(t('app.toast.check_email'));
      return;
    }
    await completeLogin(r.handle, password); // verification off — logged straight in
    return;
  }
  try {
    const r = await api('/api/login','POST',{handle, password});
    await completeLogin(r.handle, password);
  } catch(e) {
    // An unverified account can't log in yet — offer to resend the link.
    if ((e.message||'') === t('err.email_unverified')) {
      S.pendingVerifyHandle = handle;
      $('verifyNotice').style.display='block';
    }
    throw e; // let guard surface the message as a toast
  }
});
// Re-send the verification email for the handle in the field (or the one we just
// signed up / tried to log in with). Always an opaque success — the server never
// reveals whether the account exists or is already verified.
const resendVerify = guard(async () => {
  const handle = ($('handleInput').value.trim()) || S.pendingVerifyHandle;
  if (!handle) { toast(t('app.toast.enter_handle_pw'),'err'); return; }
  await api('/api/resend','POST',{handle});
  toast(t('app.toast.resent'));
});
const logout = guard(async () => { await api('/api/logout','POST'); location.reload(); });
let WS_STARTED = false;
function afterLogin() {
  $('overlay').style.display='none';
  $('meHandle').textContent = S.me;
  loadServers(); loadSocial();
  if (!WS_STARTED) { WS_STARTED = true; connectWS(); } // WS requires a session
}

/* ── Mode switch ────────────────────────────────────────────────── */
function setMode(m){
  S.mode = m;
  $('tabChat').classList.toggle('on', m==='chat');
  $('tabDms').classList.toggle('on', m==='dms');
  $('app').className = m;
  $('chatScreen').classList.toggle('hidden', m!=='chat');
  $('dmScreen').classList.toggle('hidden', m!=='dms');
  if(m==='dms') loadSocial();
}

/* ── Servers / channels (chat) ──────────────────────────────────── */
async function loadServers() {
  const gs = await api('/api/servers');
  $('serverList').innerHTML = '';
  gs.forEach(g => {
    const d = document.createElement('div');
    d.className = 'item' + (g.slug===S.server ? ' sel':'');
    d.innerHTML = `${avatar(g.name)}<div class="t grow"><div class="nm">${esc(g.name)}</div><div class="sub">${g.phase} · ${t('app.server.citizens',{n:g.citizens})}</div></div>`;
    d.onclick = () => selectServer(g.slug);
    $('serverList').appendChild(d);
  });
  if (!S.server && gs.length) selectServer(gs[0].slug);
  else if (!gs.length) showServerHome();
}

/* The "no server yet" home: a new member's three ways in. */
function showServerHome(){
  S.server = null;
  $('channelsHeading').textContent = t('app.channels.heading');
  $('channelList').innerHTML = '';
  $('addChannelBtn').style.display = 'none';
  $('messages').innerHTML = `<div class="empty home">
      <div class="big">${icon('vote',40)}</div>
      <h3>${t('app.home.welcome')}</h3>
      <div class="sub">${t('app.home.sub')}</div>
      <div class="homeacts">
        <button data-act="browseServers">${t('app.home.browse')}</button>
        <button data-act="joinByCode">${t('app.home.join_code')}</button>
        <button class="primary" data-act="newServer">${t('app.found.title')}</button>
      </div>
    </div>`;
}

const newServer = guard(async () => {
  const v = await modal({title:t('app.found.title'), fields:[
    {name:'name',label:t('app.found.name_label'),placeholder:t('app.found.name_ph')},
    {name:'visibility',label:t('app.found.visibility'),type:'select',value:'public',options:[
      {value:'public',label:t('app.found.public')},
      {value:'private',label:t('app.found.private')}]}],
    submitText:t('app.found.submit')});
  if(!v||!v.name) return;
  const g = await api('/api/servers','POST',{name:v.name, is_private: v.visibility==='private'});
  S.server=g.slug; await loadServers(); selectServer(g.slug);
  toast(t('app.toast.founded',{name:g.name}), 'ok');
});

/* Browse the public directory and join any listed server. */
const browseServers = guard(async () => {
  const gs = await api('/api/servers/public');
  S.server = null;
  $('channelsHeading').textContent = t('app.channels.heading'); $('channelList').innerHTML=''; $('addChannelBtn').style.display='none';
  const rows = gs.length ? gs.map(g=>`
      <div class="browserow">
        <div class="grow"><div class="nm">${esc(g.name)}</div>
          <div class="sub">${g.phase} · ${t('app.server.citizens',{n:g.citizens})}</div></div>
        <button class="sm" data-act="joinPublic" data-slug="${esc(g.slug)}">${t('app.btn.join')}</button>
      </div>`).join('') : `<div class="sub">${t('app.browse.empty')}</div>`;
  $('messages').innerHTML = `<div class="browse"><h3>${t('app.browse.title')}</h3>${rows}</div>`;
});
const joinPublic = guard(async (slug) => {
  await api('/api/servers/'+enc(slug)+'/join','POST');
  await loadServers(); selectServer(slug); toast(t('app.toast.joined'), 'ok');
});

/* Redeem an invite code to join a (possibly private) server. */
const joinByCode = guard(async () => {
  const v = await modal({title:t('app.home.join_code'),
    fields:[{name:'code',label:t('app.join.code_label'),placeholder:t('app.join.code_ph')}], submitText:t('app.btn.join')});
  if(!v||!v.code) return;
  const r = await api('/api/invites/accept','POST',{code:v.code.trim()});
  await loadServers(); if(r.slug) selectServer(r.slug);
  toast(t('app.toast.joined'), 'ok');
});

/* Mint an invite code for the current server and show it to copy. Members only,
   and only while the server's invite policy is open. */
const inviteToServer = guard(async () => {
  if(!S.server) return;
  const r = await api('/api/servers/'+enc(S.server)+'/invites','POST');
  await modal({title:t('app.invite.title'), message:t('app.invite.message'),
    fields:[{name:'code',label:t('app.invite.code_label'),value:r.code}], submitText:t('app.btn.done')});
});
async function selectServer(slug) {
  S.server = slug; S.channel = null; S.reply = null;
  const d = await api('/api/servers/'+slug); S.serverDetail = d;
  $('channelsHeading').textContent = d.name + (d.is_private ? ' ·' : '');
  $('addChannelBtn').style.display = (d.phase==='Seed' && d.founder===S.me) ? 'block':'none';
  $('inviteBtn').style.display = d.are_invites_open ? 'block' : 'none';
  $('serverSettingsBtn').style.display = 'block';
  S.channels = d.channels;
  $('channelList').innerHTML = '';
  d.channels.forEach(c => {
    const el = document.createElement('div');
    el.className='item'+(c.name===S.channel?' sel':'');
    const lock = c.is_encrypted ? ` <span class="clock lockic" title="${t('app.e2ee.tooltip')}">${icon('lock',12)}</span>` : '';
    el.innerHTML = `<div class="t grow"><div class="nm"># ${esc(c.name)}${lock}</div></div>`;
    el.onclick = () => selectChannel(c.name);
    $('channelList').appendChild(el);
  });
  $('surface').innerHTML = d.surface.map(s=>`<span>${esc(s)}</span>`).join('');
  loadServers(); loadMe(); loadProposals(); loadRoles(); loadMentionable(); loadEmojis();
  if (d.channels.length) selectChannel(d.channels[0].name);
  else $('messages').innerHTML = `<div class="empty"><div class="big">#</div><div>${t('app.channels.none')}</div></div>`;
}
const newChannel = guard(async () => {
  const v = await modal({title:t('app.newchan.title'), fields:[
    {name:'name',label:t('app.field.name'),placeholder:t('app.newchan.name_ph')},
    {name:'topic',label:t('app.field.topic_optional'),placeholder:t('app.newchan.topic_ph')}], submitText:t('app.btn.create')});
  if(!v||!v.name) return;
  await api('/api/servers/'+S.server+'/channels','POST',{name:v.name,topic:v.topic||''}); selectServer(S.server);
});
/* Load and open this channel's key grants into `S.chanKeys[name]` (epoch → key hex),
   so sealed messages can be decrypted and new ones sealed. No-op for plaintext
   channels or when the device identity isn't loaded. */
async function loadChannelKeys(name){
  S.chanKeys[name] = {};
  const ch=(S.channels||[]).find(c=>c.name===name);
  if(!ch||!ch.is_encrypted||!S.ident) return;
  await crypto_();
  let grants=[]; try { grants = await api(`/api/servers/${S.server}/channels/${enc(name)}/keys`); } catch {}
  grants.forEach(g=>{ try { S.chanKeys[name][g.epoch] = E.open(S.ident.secret, g.sealed_key); } catch {} });
}

async function selectChannel(name){ S.channel=name; S.reply=null; setReplyBar();
  document.body.classList.remove('nav-open'); // close the mobile drawer on pick
  document.querySelectorAll('#channelList .item .nm').forEach(el=>el.parentElement.parentElement.classList.toggle('sel', el.textContent.startsWith('# '+name)));
  await loadChannelKeys(name);
  const ch=(S.channels||[]).find(c=>c.name===name);
  const isEnc = !!(ch&&ch.is_encrypted);
  const haveKey = isEnc && Object.keys(S.chanKeys[name]||{}).length>0;
  const inp=$('msgInput');
  inp.disabled = isEnc && !haveKey;
  inp.placeholder = !isEnc ? t('app.compose.placeholder')
    : haveKey ? t('app.compose.enc_placeholder')
    : (S.ident ? t('app.compose.no_key') : t('app.compose.locked'));
  updateAttachUI();
  loadMessages(); }

/* Highlight resolved @mentions and expand :custom_emoji: inside an escaped body.
   A custom `:name:` renders as its image — jumbo when the whole message is nothing
   but custom emoji, inline text-size when mixed with any other text. Only names in
   this server's retained emoji map are touched; unknown `:tokens:` and unicode
   emoji are left exactly as typed. */
function renderBody(m){
  // A sealed message (key_epoch set) is ciphertext. Open it with the channel key
  // for its epoch if we hold one; otherwise show it locked.
  if(m.key_epoch!=null){
    const key = (S.chanKeys[S.channel]||{})[m.key_epoch];
    if(S.ident && E && key){
      try { return renderText(hexToText(E.channel_open(key, m.body)), m); } catch {}
    }
    const why = S.ident ? t('app.sealed.no_key') : t('app.sealed.sign_in');
    return `<span class="sealed" title="${t('app.sealed.tooltip',{epoch:m.key_epoch})}">${icon('lock',13)} <span class="sub">${t('app.sealed.encrypted')} ${why}</span></span>`;
  }
  return renderText(m.body, m);
}

/* Escape `body`, then highlight resolved @mentions and expand :custom_emoji:. Used
   for plaintext messages and for decrypted channel messages alike (a decrypted body
   carries no server-resolved mentions, so those simply render as plain text). */
function renderText(body, m){
  const kinds = {}; (m.mentions||[]).forEach(x=>kinds[x.token]=x.kind);
  const map = S.emojiMap || {};
  const EMO = /:([A-Za-z0-9_-]+):/g;

  // Jumbo only if every non-whitespace character belongs to a known :emoji:.
  let count = 0;
  const leftover = body.replace(EMO, (full, name)=>{
    if(map[name.toLowerCase()]!==undefined){ count++; return ''; } return full;
  }).trim();
  const jumbo = count>0 && leftover==='';

  let s = esc(body);

  // Stash fixed, already-safe HTML fragments (code spans, autolinks) behind NUL
  // placeholders so the later markdown / mention / emoji passes can't reach inside
  // them and corrupt an href or re-format code. Restored verbatim at the very end.
  const stash = [];
  const hold = html => `\uE000${stash.push(html)-1}\uE000`;

  s = s
    // Fenced code block ```…``` → <pre><code>. Content is already escaped.
    .replace(/```\n?([\s\S]*?)```/g, (_m, code)=> hold(`<pre><code>${code.replace(/\n$/,'')}</code></pre>`))
    // Inline code `…`
    .replace(/`([^`\n]+)`/g, (_m, code)=> hold(`<code>${code}</code>`))
    // Bare http/https URLs (strip trailing sentence punctuation back out).
    .replace(/https?:\/\/[^\s<\uE000]+/g, (u)=>{
      let tail=''; const p=/[.,!?:;)\]}'"]+$/.exec(u); if(p){ tail=p[0]; u=u.slice(0, -tail.length); }
      return hold(`<a href="${u}" target="_blank" rel="noopener noreferrer">${u}</a>`)+tail;
    })
    // Blockquote: a line starting "> " (escaped to "&gt; ").
    .replace(/^&gt; ?(.*)$/gm, (_m, txt)=>`<blockquote>${txt}</blockquote>`)
    // Spoiler ||…|| → click-to-reveal span.
    .replace(/\|\|([\s\S]+?)\|\|/g, (_m, txt)=>`<span class="spoiler" data-act="revealSpoiler" title="${t('app.spoiler.reveal')}">${txt}</span>`);

  s = mdInline(s)
    .replace(/@([A-Za-z0-9_-]+)/g, (full, tok)=>{
      const k = kinds[tok.toLowerCase()]; if(!k) return full;
      const cls = k==='user' ? 'm-user' : 'm-role';
      return `<span class="mention ${cls}">@${esc(tok)}</span>`;
    })
    .replace(EMO, (full, name)=>{
      const url = map[name.toLowerCase()]; if(url===undefined) return full;
      return `<img class="cemoji${jumbo?' jumbo':''}" src="${esc(url)}" alt=":${esc(name)}:" title=":${esc(name)}:">`;
    });

  return s.replace(/\uE000(\d+)\uE000/g, (_m, i)=> stash[+i]);
}

/* Slack-style inline emphasis over an already-escaped string. Only fixed tags are
   inserted; the captured text is user content that was escaped upstream. */
function mdInline(s){
  return s
    .replace(/\*([^*\n]+)\*/g, '<strong>$1</strong>')
    .replace(/_([^_\n]+)_/g, '<em>$1</em>')
    .replace(/~([^~\n]+)~/g, '<del>$1</del>');
}

/* Render a message's media attachments (images/video/audio) beneath its body.
   Spoilered media is blurred behind a click-to-reveal overlay. */
function renderAttachments(m){
  const list = m.attachments || []; if(!list.length) return '';
  const items = list.map(a=>{
    const cap = a.caption ? `<div class="att-cap">${esc(a.caption)}</div>` : '';
    let inner;
    if(a.kind==='image'){
      const open = a.is_spoiler ? '' : ` data-act="openMedia" data-url="${esc(a.url)}"`;
      inner = `<img loading="lazy" src="${esc(a.url)}" alt="${esc(a.caption||'')}"${open}>`;
    } else if(a.kind==='video'){
      inner = `<video controls preload="metadata" src="${esc(a.url)}"></video>`;
    } else if(a.kind==='audio'){
      inner = `<audio controls preload="metadata" src="${esc(a.url)}"></audio>`;
    } else {
      inner = `<a href="${esc(a.url)}" target="_blank" rel="noopener noreferrer">${esc(a.caption||a.url)}</a>`;
    }
    if(a.is_spoiler){
      return `<div class="att">${cap}<div class="spoiler-media" data-act="revealMedia"><span class="sm-label">${t('app.spoiler.reveal')}</span>${inner}</div></div>`;
    }
    return `<div class="att">${cap}${inner}</div>`;
  }).join('');
  return `<div class="attachments">${items}</div>`;
}

async function loadMessages() {
  if (!S.server || !S.channel) return;
  const msgs = await api(`/api/servers/${S.server}/channels/${S.channel}/messages`);
  const box = $('messages'); box.innerHTML='';
  const ch=(S.channels||[]).find(c=>c.name===S.channel);
  const head = document.createElement('div'); head.id='channelTitle'; head.className='chan-head';
  // Encryption control: citizens can turn on E2EE for a plaintext channel, or
  // re-share the key with members (e.g. newcomers) once it is encrypted.
  let ctl='';
  if(S.tier==='citizen' && S.ident){
    ctl = ch&&ch.is_encrypted
      ? `<button class="ghost sm iconrow" data-act="shareKeys" title="${t('app.chan.reshare_title')}">${icon('lock',14)} ${t('app.chan.reshare_keys')}</button>`
      : `<button class="ghost sm iconrow" data-act="encryptChannel" title="${t('app.chan.encrypt_title')}">${icon('lock',14)} ${t('app.chan.encrypt')}</button>`;
  }
  head.innerHTML = `<h3># ${esc(S.channel)}${ch&&ch.is_encrypted?` <span class="clock lockic" title="${t('app.e2ee.tooltip')}">${icon('lock',13)}</span>`:''}</h3>${ctl}`;
  box.appendChild(head);
  // Messages live in a wrapper with margin-top:auto so the list hugs the bottom.
  const list = document.createElement('div'); list.id='msgList'; box.appendChild(list);
  if(!msgs.length){ const e=document.createElement('div'); e.className='sub'; e.style.marginTop='.6rem'; e.textContent=t('app.msgs.empty'); list.appendChild(e); }
  msgs.forEach(m => {
    const el = document.createElement('div'); el.id='msg-'+m.id;
    el.className='msg-group'+(m.mentions_me?' mentions-me':'');
    const chips = m.reactions.map(([e,n])=>`<span class="chip" data-act="react" data-id="${m.id}" data-emoji="${esc(e)}">${e} ${n}</span>`).join('');
    const ref = m.reply_to ? `<div class="replyref" data-act="jumpTo" data-id="${m.reply_to.id}">
        <span class="rr-arrow">${icon('reply',13)}</span><span class="rr-who">@${esc(m.reply_to.author)}</span>
        <span class="rr-ex">${esc(m.reply_to.excerpt)}</span></div>` : '';
    el.innerHTML = `${ref}
      <div class="msg">
        ${avatar(m.author,'sm')}
        <div class="bubblewrap">
          <span class="when compact-date">${fmtDate(m.ts)}</span>
          <div class="hdr">
            <span class="who">${esc(m.author)}</span>
            <span class="when time">${fmtTime(m.ts)}</span>
            ${m.edited?`<span class="when">${t('app.msg.edited')}</span>`:''}
            <span class="tools">
              <button class="sm ghost" data-act="setReply" data-id="${m.id}" data-who="${esc(m.author)}">${t('app.msg.reply')}</button>
              <button class="sm ghost" data-act="quickReact" data-id="${m.id}">${t('app.msg.react')}</button>
            </span>
          </div>
          <div class="body">${renderBody(m)}</div>
          ${renderAttachments(m)}
          ${chips?`<div class="reacts">${chips}</div>`:''}
        </div>
      </div>`;
    list.appendChild(el);
  });
  box.scrollTop = box.scrollHeight;
}

/* A tiny date+time line for compact mode, e.g. "Jul 14, 12:34 PM". */
function fmtDate(ts){
  if(!ts) return '';
  return new Date(ts*1000).toLocaleString([], {month:'short', day:'numeric', hour:'numeric', minute:'2-digit'});
}

/* Format an epoch-seconds timestamp as a short local time, with the date if it
   isn't today. */
function fmtTime(ts){
  if(!ts) return '';
  const d = new Date(ts*1000), now = new Date();
  const t = d.toLocaleTimeString([], {hour:'numeric', minute:'2-digit'});
  const sameDay = d.toDateString()===now.toDateString();
  return sameDay ? t : d.toLocaleDateString([], {month:'short', day:'numeric'})+' '+t;
}

/* Scroll to and briefly highlight the original of a reply. */
function jumpTo(id){
  const el = document.getElementById('msg-'+id); if(!el) return;
  el.scrollIntoView({behavior:'smooth', block:'center'});
  el.classList.remove('flash'); void el.offsetWidth; el.classList.add('flash');
  setTimeout(()=>el.classList.remove('flash'), 1200);
}

function roleOptions(){ return (S.customRoles||[]).map(n=>({value:n,label:'@'+n})); }

async function loadRoles(){
  if(!S.server) return;
  const roles = await api(`/api/servers/${S.server}/roles`);
  S.customRoles = roles.map(r=>r.name);
  const box=$('roles');
  const builtin = `<div class="card sub"><b>${t('app.roles.builtin')}</b> @everyone · @members · @citizens</div>`;
  if(!roles.length){ box.innerHTML = builtin + `<div class="sub" style="margin-bottom:.6rem">${t('app.roles.none')}</div>`; return; }
  box.innerHTML = builtin + roles.map(r=>`
    <div class="card" style="padding:.55rem .7rem">
      <div><span class="mention m-role">@${esc(r.name)}</span></div>
      <div class="sub" style="margin-top:.3rem">${r.holders.length ? r.holders.map(h=>'@'+esc(h)).join(', ') : t('app.roles.no_members')}</div>
    </div>`).join('');
}

async function loadMentionable(){
  if(!S.server){ S.mentionable={users:[],roles:[]}; return; }
  try { S.mentionable = await api(`/api/servers/${S.server}/mentionable`); } catch { S.mentionable={users:[],roles:[]}; }
}
function setReply(id, who){ S.reply={id,who}; setReplyBar(); $('msgInput').focus(); }
function setReplyBar(){ const b=$('replybar'); if(S.reply){ b.style.display='block';
  b.innerHTML = `${t('app.reply.to')} <b>${esc(S.reply.who)}</b> · <a href="#" data-act="cancelReply">${t('app.reply.cancel')}</a>`; }
  else b.style.display='none'; }
/* ── @-mention autocomplete ─────────────────────────────────────── */
const AC = { open:false, items:[], idx:0, start:0 };
function acClose(){ AC.open=false; $('acPopup').style.display='none'; }
function acRender(){
  const p=$('acPopup');
  p.innerHTML = AC.items.map((it,i)=>`<div class="ac ${i===AC.idx?'on':''}" data-i="${i}">
     <span>@${esc(it.name)}</span><span class="k">${it.role?t('app.ac.role'):t('app.ac.user')}</span></div>`).join('');
  p.style.display = AC.items.length ? 'block' : 'none';
}
function acOnInput(){
  const inp=$('msgInput'); const pos=inp.selectionStart;
  const m = inp.value.slice(0,pos).match(/@([A-Za-z0-9_-]*)$/);
  if(!m){ acClose(); return; }
  AC.start = pos - m[0].length;
  const q = m[1].toLowerCase();
  const roles = (S.mentionable.roles||[]).filter(r=>r.toLowerCase().startsWith(q)).map(name=>({name,role:true}));
  const users = (S.mentionable.users||[]).filter(u=>u.toLowerCase().startsWith(q) && u!==S.me).map(name=>({name,role:false}));
  AC.items = [...roles, ...users].slice(0,8); AC.idx=0; AC.open=AC.items.length>0; acRender();
}
function acPick(i){
  const it=AC.items[i]; if(!it) return;
  const inp=$('msgInput'); const pos=inp.selectionStart;
  const before=inp.value.slice(0,AC.start), after=inp.value.slice(pos);
  inp.value = before+'@'+it.name+' '+after;
  const caret=(before+'@'+it.name+' ').length; inp.setSelectionRange(caret,caret);
  acClose(); inp.focus();
}
/* Returns true if the key was consumed by the popup. */
function acKeydown(e){
  if(!AC.open) return false;
  if(e.key==='ArrowDown'){ e.preventDefault(); AC.idx=(AC.idx+1)%AC.items.length; acRender(); return true; }
  if(e.key==='ArrowUp'){ e.preventDefault(); AC.idx=(AC.idx-1+AC.items.length)%AC.items.length; acRender(); return true; }
  if(e.key==='Enter'||e.key==='Tab'){ e.preventDefault(); acPick(AC.idx); return true; }
  if(e.key==='Escape'){ acClose(); return true; }
  return false;
}
$('msgInput').addEventListener('input', acOnInput);
$('msgInput').addEventListener('keydown', e=>{ if(acKeydown(e)) return; if(e.key==='Enter') sendMessage(); });
$('acPopup').addEventListener('mousedown', e=>{ const el=e.target.closest('.ac'); if(el){ e.preventDefault(); acPick(+el.dataset.i); } });

/* ── Composer media attachments ──────────────────────────────────────
   Files the user has picked but not yet sent. Each: {file, kind, url, is_spoiler}
   where `url` is an object-URL preview for images (revoked on removal/send). */
let PENDING = [];

function pendingKind(type){
  return type.startsWith('image/') ? 'image'
       : type.startsWith('video/') ? 'video'
       : type.startsWith('audio/') ? 'audio' : 'file';
}
function addPending(file){
  const kind = pendingKind(file.type||'');
  const url = kind==='image' ? URL.createObjectURL(file) : null;
  PENDING.push({file, kind, url, is_spoiler:false});
  renderPending();
}
function clearPending(){
  PENDING.forEach(p=>{ if(p.url) URL.revokeObjectURL(p.url); });
  PENDING = []; renderPending();
}
/* Render pending files as chips above the input: image → thumbnail, other → kind
   icon + filename; each with a spoiler toggle and a remove (×). */
function renderPending(){
  const box = $('pendingAtts'); if(!box) return;
  box.innerHTML = PENDING.map((p,i)=>{
    const thumb = p.kind==='image' && p.url
      ? `<img class="pa-thumb" src="${p.url}" alt="">`
      : `<span class="pa-ic">${icon(p.kind==='video'?'film':p.kind==='audio'?'music':'attach',16)}</span>`;
    const spLabel = p.is_spoiler ? t('app.attach.spoiler_on') : t('app.attach.spoiler_off');
    return `<div class="pa-chip">${thumb}<span class="pa-name">${esc(p.file.name)}</span>`+
      `<button class="pa-sp${p.is_spoiler?' on':''}" data-act="toggleAttSpoiler" data-i="${i}" title="${t('app.attach.spoiler_toggle')}">${spLabel}</button>`+
      `<button class="pa-x" data-act="removeAtt" data-i="${i}" title="${t('app.attach.remove')}" aria-label="${t('app.attach.remove')}">×</button></div>`;
  }).join('');
  box.style.display = PENDING.length ? 'flex' : 'none';
}
/* Show/hide the attach button for the current channel: attachments are only for
   plaintext channels. Clears any pending files when moving to an encrypted one. */
function updateAttachUI(){
  const btn = $('attachBtn'); if(!btn) return;
  const enc_ = !!((S.channels||[]).find(c=>c.name===S.channel)?.is_encrypted);
  btn.style.display = enc_ ? 'none' : '';
  if(enc_ && PENDING.length) clearPending();
}
const fileInput = $('attachInput');
if(fileInput) fileInput.addEventListener('change', e=>{
  for(const f of e.target.files) addPending(f);
  e.target.value = ''; // reset so the same file can be re-picked
});

const sendMessage = guard(async () => {
  const input=$('msgInput'); const body=input.value.trim();
  const ch=(S.channels||[]).find(c=>c.name===S.channel);
  if(ch&&ch.is_encrypted){
    if(!body) return;
    if(!S.ident){ toast(t('app.toast.unlock_channels'),'err'); return; }
    const keys=S.chanKeys[S.channel]||{}; const epochs=Object.keys(keys).map(Number);
    if(!epochs.length){ toast(t('app.toast.no_channel_key'),'err'); return; }
    await crypto_();
    const epoch=Math.max(...epochs); // seal under the newest key we hold
    const ciphertext=E.channel_seal(keys[epoch], textToHex(body));
    await api(`/api/servers/${S.server}/channels/${enc(S.channel)}/sealed-messages`,'POST',{ciphertext, key_epoch:epoch, parent:S.reply?S.reply.id:null});
    input.value=''; S.reply=null; setReplyBar(); acClose(); loadMessages(); return;
  }
  // Attachments only on top-level (non-reply) plaintext messages. Upload each raw
  // file first, then post the message referencing the returned keys.
  const canAttach = PENDING.length && !S.reply;
  if(!body && !canAttach) return;
  let attachments = [];
  if(canAttach){
    for(const p of PENDING){
      const r = await fetch('/api/media',{method:'POST',headers:{'Content-Type':p.file.type||'application/octet-stream','X-DC-CSRF-Token':csrfToken()},body:p.file});
      if(!r.ok){ toast(t((await r.text()).trim()),'err'); return; }
      const up = await r.json();
      attachments.push({key:up.key, content_type:up.content_type, caption:'', is_spoiler:p.is_spoiler});
    }
  }
  await api(`/api/servers/${S.server}/channels/${S.channel}/messages`,'POST',{body, parent:S.reply?S.reply.id:null, attachments});
  input.value=''; S.reply=null; setReplyBar(); acClose(); clearPending();
  loadMessages(); // optimistic: show my own message immediately, don't wait on the WS echo
});
const quickReact = guard(async id => {
  const v = await modal({title:t('app.react.title'), fields:[{name:'emoji',label:t('app.react.pick'),type:'emoji'}]});
  if(v&&v.emoji) react(id, v.emoji);
});

/* Seal a channel key (hex) under each member's device key and file the grants.
   Members without published keys are skipped and reported. */
async function distributeChannelKey(channel, epoch, keyHex){
  await crypto_();
  const members = (await api(`/api/servers/${S.server}/mentionable`)).users || [];
  let granted=0, skipped=0;
  for(const h of members){
    let pub; try { pub = (await api('/api/keys/'+enc(h))).public_key; } catch { skipped++; continue; }
    const sealed = E.seal(pub, keyHex);
    await api(`/api/servers/${S.server}/channels/${enc(channel)}/keys`,'POST',{epoch, member:h, sealed_key:sealed});
    granted++;
  }
  return {granted, skipped};
}

/* Turn on end-to-end encryption for the current channel, mint the first channel
   key, and share it with the current members (citizen action). */
const encryptChannel = guard(async () => {
  if(!S.channel) return;
  if(!S.ident){ toast(t('app.toast.unlock_encryption'),'err'); return; }
  const v = await modal({title:t('app.encrypt.title'), message:t('app.encrypt.message'), fields:[
    {name:'mode',label:t('app.encrypt.history_label'),type:'select',options:[
      {value:'open',label:t('app.encrypt.open')},
      {value:'ephemeral',label:t('app.encrypt.ephemeral')}]}], submitText:t('app.encrypt.submit')});
  if(!v) return;
  await crypto_();
  await api(`/api/servers/${S.server}/channels/${enc(S.channel)}/encrypt`,'POST',{history_mode:v.mode});
  const key = E.channel_key_generate();
  const {granted, skipped} = await distributeChannelKey(S.channel, 0, key);
  toast(t('app.encrypt.done',{n:granted}) + (skipped?t('app.share.skipped',{n:skipped}):'') + '.','ok');
  selectServer(S.server);
});

/* Re-share the current channel key with members (e.g. newcomers who joined after
   encryption was turned on). Uses the newest key epoch this client holds. */
const shareKeys = guard(async () => {
  if(!S.channel||!S.ident){ toast(t('app.toast.unlock_encryption'),'err'); return; }
  const keys=S.chanKeys[S.channel]||{}; const epochs=Object.keys(keys).map(Number);
  if(!epochs.length){ toast(t('app.toast.no_key_reshare'),'err'); return; }
  const epoch=Math.max(...epochs);
  const {granted, skipped} = await distributeChannelKey(S.channel, epoch, keys[epoch]);
  toast(t('app.share.done',{n:granted}) + (skipped?t('app.share.skipped',{n:skipped}):'') + '.','ok');
});
const react = guard(async (id,emoji) => { await api('/api/messages/'+id+'/react','POST',{emoji}); loadMessages(); });

/* ── Governance ─────────────────────────────────────────────────── */
async function loadProposals(){
  if(!S.server) return;
  const ps = await api(`/api/servers/${S.server}/proposals`);
  const box = $('proposals'); box.innerHTML='';
  if(!ps.length){ box.innerHTML=`<div class="sub" style="margin-bottom:.6rem">${t('app.props.none')}</div>`; }
  ps.slice().reverse().forEach(p => {
    const el = document.createElement('div'); el.className='prop';
    let statusLine;
    if(p.status==='open'){ const d=Math.max(0,Math.round(p.closes_in/86400)); statusLine=`<span class="st-open">${t('app.props.open')}</span> · ${t('app.props.closes_in',{d})}`; }
    else if(p.status==='passed'){ statusLine=`<span class="st-passed">${t('app.props.passed')}</span>${p.is_applied?' · '+t('app.props.applied'):' · '+t('app.props.applying')}`; }
    else statusLine=`<span class="st-failed">${t('app.props.failed')}</span>`;
    let controls='';
    if(p.status==='open' && S.tier==='citizen'){
      controls = `<button class="vbtn ${p.my_vote===true?'on-aye':''}" data-act="vote" data-id="${p.id}" data-aye="1">${t('app.props.aye')}</button>
                  <button class="vbtn ${p.my_vote===false?'on-nay':''}" data-act="vote" data-id="${p.id}" data-aye="0">${t('app.props.nay')}</button>`;
    }
    el.innerHTML = `<div class="head">${esc(p.summary)}</div>
      <div class="sub">${t('app.props.by')} @${esc(p.proposer)} · ${statusLine}</div>
      <div class="votes">${controls}<span class="tal">▲ ${p.aye} · ▼ ${p.nay}</span></div>`;
    box.appendChild(el);
  });
}
const vote = guard(async (id,isAye) => { await api('/api/proposals/'+id+'/vote','POST',{is_aye:isAye}); loadProposals(); });

/* ── Custom emoji (server settings) ─────────────────────────────── */
async function loadEmojis(){
  if(!S.server){ S.emojiMap={}; return; }
  // Full retained map first, so `:name:` renders even for archived emoji.
  try { S.emojiMap = await api(`/api/servers/${S.server}/emojis/map`); }
  catch { S.emojiMap = {}; }
  const box = $('emojiList'); if(!box) return;
  let list;
  try { list = await api(`/api/servers/${S.server}/emojis`); }
  catch { box.innerHTML=`<span class="sub">${t('app.emoji.unavailable')}</span>`; return; }
  if(!list.length){ box.innerHTML=`<span class="sub">${t('app.emoji.none')}</span>`; return; }
  box.innerHTML='';
  const canVote = S.tier==='citizen';
  list.forEach(e=>{
    const row=document.createElement('div'); row.className='emoji-row';
    const controls = canVote
      ? `<button class="vbtn ${e.my_vote===true?'on-aye':''}" data-act="voteEmoji" data-id="${e.id}" data-up="1">▲</button>
         <span class="sc">${e.score}</span>
         <button class="vbtn ${e.my_vote===false?'on-nay':''}" data-act="voteEmoji" data-id="${e.id}" data-up="0">▼</button>`
      : `<span class="sc">${e.score}</span>`;
    row.innerHTML = `<img src="${esc(e.url)}" alt=":${esc(e.name)}:">
      <div class="grow"><div class="en">:${esc(e.name)}: <span class="tier ${e.is_active?'active':''}">${esc(e.standing)}</span></div></div>
      ${controls}`;
    box.appendChild(row);
  });
}
const voteEmoji = guard(async (id,isUp) => { await api(`/api/servers/${S.server}/emojis/${id}/vote`,'POST',{is_up:isUp}); loadEmojis(); });
const addEmoji = guard(async () => {
  const v = await modal({title:t('app.addemoji.title'), message:t('app.addemoji.message'), fields:[
    {name:'name',label:t('app.addemoji.name_label')},
    {name:'image',label:t('app.addemoji.image_label'),type:'file',accept:'image/png,image/gif'},
    {name:'url',label:t('app.addemoji.url_label')}], submitText:t('app.btn.add')});
  if(!v||!v.name) return;
  if(!v.image && !v.url){ toast(t('app.toast.emoji_need_image'),'err'); return; }
  await api(`/api/servers/${S.server}/emojis`,'POST',{name:v.name, url:v.url||'', image:v.image||null});
  toast(t('app.toast.emoji_added'),'ok'); loadEmojis();
});
const newProposal = guard(async () => {
  const pick = await modal({title:t('app.newprop.title'), message:t('app.newprop.message'), fields:[
    {name:'kind',label:t('app.newprop.ballot_label'),type:'select',options:[
      {value:'AddRule',label:t('app.ballot.add_rule')},{value:'CreateChannel',label:t('app.ballot.create_channel')},
      {value:'DeleteChannel',label:t('app.ballot.delete_channel')},{value:'Ban',label:t('app.ballot.ban')},
      {value:'CreateRole',label:t('app.ballot.create_role')},{value:'DeleteRole',label:t('app.ballot.delete_role')},
      {value:'AssignRole',label:t('app.ballot.assign_role')},{value:'UnassignRole',label:t('app.ballot.unassign_role')},
      {value:'SetRehomingPolicy',label:t('app.ballot.rehoming_fed')}]}], submitText:t('app.btn.next')});
  if(!pick) return;
  const K = pick.kind; let body={kind:K}; let v;
  if(['DeleteRole','AssignRole','UnassignRole'].includes(K) && !roleOptions().length){ toast(t('app.toast.no_roles_first'),'err'); return; }
  if(K==='AddRule'){ v=await modal({title:t('app.ballot.add_rule'),fields:[{name:'text',label:t('app.rule.text_label'),type:'textarea'}],submitText:t('app.btn.propose')}); if(!v||!v.text) return; body.text=v.text; }
  else if(K==='CreateChannel'){ v=await modal({title:t('app.ballot.create_channel'),fields:[{name:'name',label:t('app.field.name')},{name:'topic',label:t('app.field.topic_optional')}],submitText:t('app.btn.propose')}); if(!v||!v.name) return; body.name=v.name; body.topic=v.topic||''; }
  else if(K==='DeleteChannel'){ v=await modal({title:t('app.ballot.delete_channel'),fields:[{name:'name',label:t('app.field.channel_name')}],submitText:t('app.btn.propose')}); if(!v||!v.name) return; body.name=v.name; }
  else if(K==='Ban'){ v=await modal({title:t('app.ballot.ban'),fields:[{name:'handle',label:t('app.ban.handle_label')}],submitText:t('app.btn.propose')}); if(!v||!v.handle) return; body.handle=v.handle; }
  else if(K==='CreateRole'){ v=await modal({title:t('app.ballot.create_role'),message:t('app.role.create_msg'),fields:[{name:'name',label:t('app.role.name_label')}],submitText:t('app.btn.propose')}); if(!v||!v.name) return; body.name=v.name; }
  else if(K==='DeleteRole'){ v=await modal({title:t('app.ballot.delete_role'),fields:[{name:'role',label:t('app.field.role'),type:'select',options:roleOptions()}],submitText:t('app.btn.propose')}); if(!v||!v.role) return; body.role=v.role; }
  else if(K==='AssignRole'){ v=await modal({title:t('app.ballot.assign_role'),fields:[{name:'handle',label:t('app.field.member_handle')},{name:'role',label:t('app.field.role'),type:'select',options:roleOptions()}],submitText:t('app.btn.propose')}); if(!v||!v.handle||!v.role) return; body.handle=v.handle; body.role=v.role; }
  else if(K==='UnassignRole'){ v=await modal({title:t('app.ballot.unassign_role'),fields:[{name:'handle',label:t('app.field.member_handle')},{name:'role',label:t('app.field.role'),type:'select',options:roleOptions()}],submitText:t('app.btn.propose')}); if(!v||!v.handle||!v.role) return; body.handle=v.handle; body.role=v.role; }
  else if(K==='SetRehomingPolicy'){ v=await modal({title:t('app.rehoming.title'), message:t('app.rehoming.message'), fields:[{name:'mode',label:t('app.rehoming.label'),type:'select',options:[{value:'disable',label:t('app.rehoming.disable')},{value:'enable',label:t('app.rehoming.enable')}]}],submitText:t('app.btn.propose')}); if(!v||!v.mode) return; body.is_disabled = v.mode==='disable'; }
  await api('/api/servers/'+S.server+'/proposals','POST',body); loadProposals(); toast(t('app.toast.proposal_opened'),'ok');
});

/* ── Server settings (governance config: voting aspects + trials) ──── */
/* Everything here is decided by ballot, never set unilaterally — the panel shows
   the current configuration and (for citizens) opens a proposal to change it. */

/* The full catalogue of ballot kinds a server may put on its surface, with
   human labels. `on` marks the two always-on kinds that can never be removed
   (a server must always be able to change who votes and what it votes on). */
const SURFACE_CATALOG = [
  {k:'RemoveContent',   label:'app.ballot.remove_content'},
  {k:'Ban',             label:'app.ballot.ban'},
  {k:'Timeout',         label:'app.ballot.timeout'},
  {k:'Recall',          label:'app.ballot.recall'},
  {k:'CreateChannel',   label:'app.ballot.create_channel'},
  {k:'DeleteChannel',   label:'app.ballot.delete_channel'},
  {k:'AddRule',         label:'app.ballot.add_rule'},
  {k:'RemoveRule',      label:'app.ballot.repeal_rule'},
  {k:'AmendCriteria',   label:'app.ballot.amend_criteria', on:true},
  {k:'SetJurySizing',   label:'app.ballot.set_jury'},
  {k:'SetVoteWeighting',label:'app.ballot.set_weighting'},
  {k:'SetWeightingScope',label:'app.ballot.set_scope'},
  {k:'GrantVoteWeight', label:'app.ballot.grant_weight'},
  {k:'SetGovernanceSurface',label:'app.ballot.set_surface', on:true},
  {k:'ManageRoles',     label:'app.ballot.manage_roles'},
  {k:'SetRehomingPolicy',label:'app.ballot.set_rehoming'},
  {k:'SetInvitePolicy', label:'app.ballot.set_invites'},
];
const weightingLabel = w => ({Equal:t('app.weighting.equal'), ByContribution:t('app.weighting.by_contribution'),
  ByTenure:t('app.weighting.by_tenure'), ByRole:t('app.weighting.by_role')}[w] || w);
const scopeLabel = s => ({Both:t('app.scope.both'), JuriesOnly:t('app.scope.juries_only'),
  BallotsOnly:t('app.scope.ballots_only'), None:t('app.scope.none')}[s] || s);
function jurySizingText(j){
  if(!j) return '—';
  if(j.mode==='Fixed') return t('app.jury.fixed',{post:j.post, comment:j.comment});
  const mult = bp => +(bp/10000).toFixed(2);         // basis points → 1.0 = 10000
  if(j.mode==='Sqrt') return t('app.jury.sqrt',{post:mult(j.post), comment:mult(j.comment)});
  return t('app.jury.proportional',{post:(j.post/100).toFixed(1), comment:(j.comment/100).toFixed(1)});
}

const openServerSettings = guard(async () => {
  const d = S.serverDetail; if(!d) return;
  const can = S.tier==='citizen';
  // Fetch the caller's own status for the Personal tab (history-sharing preference).
  let me = null; try { me = await api('/api/servers/'+S.server+'/me'); } catch {}
  const isMember = !!(me && me.tier && me.tier!=='guest');
  const shares = !me || me.shares_history==null ? true : me.shares_history;
  const row = (title, value, act) => `<div class="setrow">
      <div class="grow"><div class="setname">${esc(title)}</div><div class="sub">${value}</div></div>
      ${can?`<button class="ghost sm" data-set="${act}">${t('app.settings.propose_change')}</button>`:''}
    </div>`;
  const crit = t('app.settings.criteria',{age:d.min_account_age_days, mem:d.min_membership_days, con:d.min_contribution});
  // Governance tab — community-decided, changed only by ballot.
  const govPane = `<span class="msg">${can ? t('app.settings.msg_citizen') : t('app.settings.msg_guest')}</span>
    <h4 class="setsec">${t('app.settings.sec_voting')}</h4>
    ${row(t('app.settings.who_may_vote'), crit, 'criteria')}
    ${row(t('app.settings.vote_weighting'), esc(weightingLabel(d.vote_weighting)), 'weighting')}
    ${row(t('app.settings.weighting_applies'), esc(scopeLabel(d.weighting_scope)), 'scope')}
    ${row(t('app.settings.votes_on'), t('app.settings.kinds_enabled',{n:d.surface.length}), 'surface')}
    <h4 class="setsec">${t('app.settings.sec_trials')}</h4>
    ${row(t('app.settings.jury_sizing'), esc(jurySizingText(d.jury_sizing)), 'jury')}`;
  // Personal tab — this member's own per-server preferences.
  const personalPane = isMember
    ? `<span class="msg">${t('app.settings.personal_msg')}</span>
       <h4 class="setsec">${t('app.settings.sec_history')}</h4>
       <label class="switch full" style="justify-content:space-between">
         <span>${t('app.settings.history_toggle')}<br><span class="sub">${t('app.settings.history_sub')}</span></span>
         <span style="display:flex"><input type="checkbox" id="histShare" ${shares?'checked':''}><span class="track"></span></span>
       </label>`
    : `<div class="sub" style="margin-top:.6rem">${t('app.settings.personal_guest')}</div>`;
  const scrim=document.createElement('div'); scrim.id='scrim';
  scrim.innerHTML = `<div class="modal wide">
    <h3>${esc(d.name)} — ${t('app.settings.settings_word')}</h3>
    <div class="tabbar">
      <button type="button" class="tab on" data-tab="gov">${t('app.settings.tab_governance')}</button>
      <button type="button" class="tab" data-tab="personal">${t('app.settings.tab_personal')}</button>
    </div>
    <div data-pane="gov">${govPane}</div>
    <div data-pane="personal" class="hidden">${personalPane}</div>
    <div class="actions"><button class="ghost" data-close>${t('app.btn.close')}</button></div>
  </div>`;
  document.body.appendChild(scrim);
  requestAnimationFrame(()=>scrim.classList.add('show'));
  const close=()=>{ scrim.classList.remove('show'); setTimeout(()=>scrim.remove(),160); };
  scrim.querySelector('[data-close]').onclick=close;
  scrim.onclick=e=>{ if(e.target===scrim) close(); };
  scrim.querySelectorAll('[data-tab]').forEach(b=> b.onclick=()=>{
    scrim.querySelectorAll('[data-tab]').forEach(x=>x.classList.toggle('on', x===b));
    scrim.querySelectorAll('[data-pane]').forEach(p=>p.classList.toggle('hidden', p.dataset.pane!==b.dataset.tab));
  });
  const flows = {criteria:proposeCriteria, weighting:proposeVoteWeighting,
    scope:proposeWeightingScope, surface:proposeSurface, jury:proposeJurySizing};
  scrim.querySelectorAll('[data-set]').forEach(b=> b.onclick=()=>{ close(); flows[b.dataset.set](); });
  const hs = scrim.querySelector('#histShare');
  if(hs) hs.onchange = guard(async ()=>{
    await api('/api/servers/'+S.server+'/history-sharing','POST',{shares:hs.checked});
    toast(t(hs.checked?'app.toast.history_shared':'app.toast.history_hidden'),'ok');
  });
});

async function submitProposal(body){
  await api('/api/servers/'+S.server+'/proposals','POST',body);
  loadProposals(); toast(t('app.toast.proposal_opened_vote'),'ok');
}
const proposeCriteria = guard(async () => {
  const d=S.serverDetail;
  const v=await modal({title:t('app.ballot.amend_criteria'), message:t('app.criteria.message'), fields:[
    {name:'age',label:t('app.criteria.age_label'),value:String(d.min_account_age_days)},
    {name:'mem',label:t('app.criteria.mem_label'),value:String(d.min_membership_days)},
    {name:'con',label:t('app.criteria.con_label'),value:String(d.min_contribution)}], submitText:t('app.btn.propose')});
  if(!v) return;
  await submitProposal({kind:'AmendCriteria', min_account_age_days:Number(v.age)||0,
    min_membership_days:Number(v.mem)||0, min_contribution:Number(v.con)||0});
});
const proposeVoteWeighting = guard(async () => {
  const v=await modal({title:t('app.ballot.set_weighting'), message:t('app.weighting.message'), fields:[
    {name:'scheme',label:t('app.weighting.scheme_label'),type:'select',value:S.serverDetail.vote_weighting,options:[
      {value:'Equal',label:t('app.weighting.equal')},{value:'ByContribution',label:t('app.weighting.by_contribution')},
      {value:'ByTenure',label:t('app.weighting.by_tenure')},{value:'ByRole',label:t('app.weighting.by_role')}]}], submitText:t('app.btn.propose')});
  if(!v||!v.scheme) return;
  await submitProposal({kind:'SetVoteWeighting', scheme:v.scheme});
});
const proposeWeightingScope = guard(async () => {
  const v=await modal({title:t('app.ballot.set_scope'), message:t('app.scope.message'), fields:[
    {name:'scope',label:t('app.scope.applies_label'),type:'select',value:S.serverDetail.weighting_scope,options:[
      {value:'Both',label:t('app.scope.both')},{value:'JuriesOnly',label:t('app.scope.juries_only')},
      {value:'BallotsOnly',label:t('app.scope.ballots_only')},{value:'None',label:t('app.scope.none')}]}], submitText:t('app.btn.propose')});
  if(!v||!v.scope) return;
  await submitProposal({kind:'SetWeightingScope', scope:v.scope});
});
const proposeJurySizing = guard(async () => {
  const j=S.serverDetail.jury_sizing||{mode:'Sqrt',post:10000,comment:5000};
  const v=await modal({title:t('app.jury.title'), message:t('app.jury.message'), fields:[
    {name:'mode',label:t('app.jury.mode_label'),type:'select',value:j.mode,options:[
      {value:'Sqrt',label:t('app.jury.opt_sqrt')},{value:'Proportion',label:t('app.jury.opt_proportion')},
      {value:'Fixed',label:t('app.jury.opt_fixed')}]},
    {name:'post',label:t('app.jury.post_label'),value:String(j.post)},
    {name:'comment',label:t('app.jury.comment_label'),value:String(j.comment)}], submitText:t('app.btn.propose')});
  if(!v||!v.mode) return;
  await submitProposal({kind:'SetJurySizing', mode:v.mode, post:Number(v.post)||0, comment:Number(v.comment)||0});
});
const proposeSurface = guard(async () => {
  const enabled = new Set(S.serverDetail.surface);
  const scrim=document.createElement('div'); scrim.id='scrim';
  const rows = SURFACE_CATALOG.map(c=>`<label class="switch full" style="justify-content:space-between;margin-bottom:.5rem">
      <span>${esc(t(c.label))}${c.on?` <span class="sub">${t('app.surface.always_on')}</span>`:''}</span>
      <span style="display:flex"><input type="checkbox" data-kind="${c.k}" ${enabled.has(c.k)||c.on?'checked':''} ${c.on?'disabled':''}><span class="track"></span></span>
    </label>`).join('');
  scrim.innerHTML = `<form class="modal wide"><h3>${t('app.settings.votes_on')}</h3>
    <span class="msg">${t('app.surface.message')}</span>
    ${rows}
    <div class="actions"><button type="button" class="ghost" data-cancel>${t('app.btn.cancel')}</button><button type="submit">${t('app.btn.propose')}</button></div>
  </form>`;
  document.body.appendChild(scrim);
  requestAnimationFrame(()=>scrim.classList.add('show'));
  const close=v=>{ scrim.classList.remove('show'); setTimeout(()=>scrim.remove(),160); return v; };
  scrim.querySelector('[data-cancel]').onclick=()=>close();
  scrim.onclick=e=>{ if(e.target===scrim) close(); };
  scrim.querySelector('form').onsubmit=guard(async e=>{ e.preventDefault();
    const kinds=[...scrim.querySelectorAll('[data-kind]:checked')].map(i=>i.dataset.kind);
    close(); await submitProposal({kind:'SetGovernanceSurface', kinds});
  });
});

async function loadMe(){
  if(!S.server) return;
  const me = await api(`/api/servers/${S.server}/me`);
  S.tier = me.tier;
  $('newProposalBtn').style.display = me.tier==='citizen' ? 'block' : 'none';
  $('addEmojiBtn').style.display = me.tier==='citizen' ? 'block' : 'none';
  const d = S.serverDetail;
  let html = `<span class="pill ${me.tier}">${me.tier}</span> `;
  if (me.tier==='guest') {
    html += `<div style="margin-top:.6rem">${t('app.me.not_member')}</div><button class="full" style="margin-top:.6rem" data-act="join">${t('app.me.join_server',{name:esc(d.name)})}</button>`;
  } else if (me.tier==='citizen') {
    html += `<div style="margin-top:.6rem">${t('app.me.can_vote')}</div>`;
    html += `<div class="sub" style="margin-top:.5rem">${t('app.me.contribution',{n:me.contribution})}</div>`;
  } else {
    const pct = Math.min(100, Math.round(100*me.contribution/Math.max(1,d.min_contribution)));
    html += `<div class="sub" style="margin-top:.6rem">${t('app.me.contribution_of',{n:me.contribution, min:d.min_contribution})}</div>`;
    html += `<div class="bar"><i style="width:${pct}%"></i></div>`;
    if (me.is_eligible) html += `<button class="full" style="margin-top:.4rem" data-act="becomeCitizen">${t('app.me.become_citizen')}</button>`;
    else html += `<div class="sub" style="margin-top:.4rem">${t('app.me.to_earn')}<ul class="reasons">${me.unmet.map(u=>'<li>'+esc(u)+'</li>').join('')}</ul></div>`;
    html += `<div class="sub" style="margin-top:.5rem">${t('app.me.raise_hint')}</div>`;
    if (S.isDev) html += `<button class="ghost sm full" style="margin-top:.5rem" data-act="devEndorse">${t('app.me.dev_endorse')}</button>`;
  }
  $('govBody').innerHTML = html;
  loadEmojis(); // re-render the vote list now that tier is known
}
const join = guard(async () => { await api('/api/servers/'+S.server+'/join','POST',{}); selectServer(S.server); });
const becomeCitizen = guard(async () => { const r=await api('/api/servers/'+S.server+'/enfranchise','POST',{}); toast(r.message, r.is_admitted?'ok':''); selectServer(S.server); });
const devEndorse = guard(async () => { await api('/api/servers/'+S.server+'/dev/endorse','POST',{}); loadMe(); });
const advance = guard(async days => { const r=await api('/api/dev/advance','POST',{days}); $('clockNow').textContent='now: '+new Date(r.now*1000).toISOString().slice(0,10); loadMe(); });

/* ── Direct messages / social ───────────────────────────────────── */
async function loadSocial(){
  if(!S.me) return;
  const s = await api('/api/social/'+enc(S.me)); S.social = s;
  $('foToggle').checked = s.is_friends_only;

  // Badges: pending friend-request count, on the sidebar tab, the Friends nav, and
  // the Pending sub-tab.
  const n = s.incoming_requests.length;
  for(const id of ['dmBadge','friendsNavBadge','pendingTabBadge']){
    const b=$(id); if(!b) continue; b.textContent=n; b.classList.toggle('hidden', n===0);
  }

  // Direct-messages sidebar list.
  const pl = $('partnerList'); pl.innerHTML='';
  if(!s.partners.length) pl.innerHTML = `<div class="sub" style="padding:.2rem .4rem">${t('app.social.no_convos')}</div>`;
  s.partners.forEach(p=>{
    const el=document.createElement('div'); el.className='item'+(p.handle===S.peer?' sel':'');
    const tags = [p.is_friend?t('app.rel.friend'):'', p.is_blocked?t('app.rel.blocked'):''].filter(Boolean).join(' · ');
    el.innerHTML = `${avatar(p.handle)}<div class="t grow"><div class="nm">${esc(p.handle)}</div>${tags?`<div class="sub">${tags}</div>`:''}</div>`;
    el.onclick=()=>openDm(p.handle);
    pl.appendChild(el);
  });

  if(S.mode==='dms' && !S.peer) renderFriends();
  if(S.peer) renderRelPanel();
}

/* Discord-style Friends view: All / Pending / Blocked / Add Friend. */
function showFriends(){
  S.peer=null;
  setMode('dms');
  $('friendsView').classList.remove('hidden');
  $('convoView').classList.add('hidden');
  $('friendsNav').classList.add('on');
  document.querySelectorAll('#partnerList .item').forEach(el=>el.classList.remove('sel'));
  renderRelPanel();
  renderFriends();
}

function setFriendTab(tab){ S.friendTab=tab; renderFriends(); }

function renderFriends(){
  const s=S.social; if(!s) return;
  const tab=S.friendTab||'all';
  document.querySelectorAll('.friendsbar .ftab').forEach(b=>b.classList.toggle('on', b.dataset.tab===tab));
  const body=$('friendsBody');

  if(tab==='add'){
    body.innerHTML =
      `<div class="addfriend">
         <h3>${t('app.friends.add_title')}</h3>
         <div class="hint">${t('app.friends.add_hint')}</div>
         <div class="box">
           <input id="addFriendInput" placeholder="${t('app.friends.username_ph')}" autocomplete="off" />
           <button data-act="sendFriendRequest">${t('app.friends.send_request')}</button>
         </div>
       </div>`;
    const inp=$('addFriendInput'); if(inp){ inp.focus();
      inp.onkeydown=e=>{ if(e.key==='Enter') sendFriendRequest(); }; }
    return;
  }

  if(tab==='pending'){
    const reqs=s.incoming_requests;
    body.innerHTML = `<div class="flabel">${t('app.friends.pending',{n:reqs.length})}</div>` + (reqs.length
      ? reqs.map(h=>friendRow(h,t('app.friends.incoming'),
          `<button class="iconbtn ok" title="${t('app.friends.accept')}" data-act="acceptFriend" data-handle="${esc(h)}">${icon('check',17)}</button>`)).join('')
      : emptyState('mail',t('app.friends.no_pending_title'),t('app.friends.no_pending_sub')));
    return;
  }

  if(tab==='blocked'){
    const b=s.blocked||[];
    body.innerHTML = `<div class="flabel">${t('app.friends.blocked_count',{n:b.length})}</div>` + (b.length
      ? b.map(h=>friendRow(h,t('app.friends.blocked_status'),'')).join('')
      : emptyState('ban',t('app.friends.no_blocked_title'),t('app.friends.no_blocked_sub')));
    return;
  }

  // All friends
  const f=s.friends;
  body.innerHTML = `<div class="flabel">${t('app.friends.all',{n:f.length})}</div>` + (f.length
    ? f.map(h=>friendRow(h,t('app.friends.friend_status'),
        `<button class="iconbtn" title="${t('app.friends.message_title')}" data-act="openDmBtn" data-handle="${esc(h)}">${icon('message',17)}</button>`+
        `<button class="iconbtn bad" title="${t('app.friends.block_title')}" data-act="blockUser" data-handle="${esc(h)}">${icon('ban',17)}</button>`)).join('')
    : emptyState('people',t('app.friends.no_friends_title'),t('app.friends.no_friends_sub')));
}

function friendRow(handle, status, acts){
  return `<div class="frow">${avatar(handle,'sm')}
    <div><div class="nm">${esc(handle)}</div><div class="st">${esc(status)}</div></div>
    <div class="acts">${acts||''}</div></div>`;
}
function emptyState(ic, title, sub){
  return `<div class="femptily"><div class="big">${icon(ic,40)}</div><div style="font-weight:600">${esc(title)}</div>
          <div class="sub" style="margin-top:.25rem">${esc(sub)}</div></div>`;
}

const sendFriendRequest = guard(async () => {
  const inp=$('addFriendInput'); if(!inp) return;
  const h=inp.value.trim().replace(/^@/,''); if(!h) return;
  await api(`/api/social/${enc(S.me)}/friend/${enc(h)}`,'POST');
  toast(t('app.toast.friend_sent',{name:h}),'ok'); inp.value=''; loadSocial();
});

const startDm = guard(async () => {
  const v = await modal({title:t('app.dm.new_title'), fields:[{name:'handle',label:t('app.dm.who_label'),placeholder:t('app.dm.username_ph')}], submitText:t('app.btn.open')});
  if(v&&v.handle) openDm(v.handle.replace(/^@/,''));
});

async function openDm(handle){
  S.peer = handle;
  document.body.classList.remove('nav-open'); // close the mobile drawer on pick
  setMode('dms');
  $('friendsView').classList.add('hidden');
  $('convoView').classList.remove('hidden');
  $('friendsNav').classList.remove('on');
  document.querySelectorAll('#partnerList .item').forEach(el=>{
    const nm = el.querySelector('.nm'); el.classList.toggle('sel', nm && nm.textContent===handle); });
  const hb = $('dmHeaderBar'); hb.classList.remove('hidden');
  hb.innerHTML = `${avatar(handle,'lg')}<div class="grow"><div style="font-weight:700;font-size:1.05rem">${esc(handle)}</div></div>`;
  await loadConversation();
  renderRelPanel();
}

async function loadConversation(){
  if(!S.peer) return;
  const msgs = await api(`/api/social/${enc(S.me)}/with/${enc(S.peer)}`);
  const box=$('dmThread'); box.innerHTML='';
  if(!msgs.length){ box.innerHTML=`<div class="empty"><div class="big">${icon('message',40)}</div><div>${t('app.dm.empty',{name:esc(S.peer)})}</div></div>`; }
  const canDecrypt = !!(S.ident && E);
  msgs.forEach(m=>{
    const row=document.createElement('div'); row.className='dmrow'+(m.is_mine?' mine':'');
    // Each side opens the ciphertext sealed to its own device key.
    let text=null;
    if(canDecrypt && m.sealed_for_me){ try { text = hexToText(E.open(S.ident.secret, m.sealed_for_me)); } catch {} }
    const bubble = text==null
      ? `<div class="bubble locked" title="${t('app.e2ee.tooltip')}">${icon('lock',14)}<span class="sub">${S.ident?t('app.dm.cant_open'):t('app.sealed.sign_in')}</span></div>`
      : `<div class="bubble">${esc(text)}</div>`;
    row.innerHTML = `${m.is_mine?'':avatar(m.sender,'sm')}${bubble}`;
    box.appendChild(row);
  });
  box.scrollTop = box.scrollHeight;

  // Compose availability: E2EE needs this device's keys; blocks/friends-only still
  // gate as before.
  const p = (S.social?.partners||[]).find(x=>x.handle===S.peer);
  const blocked = p ? p.is_blocked : false;
  const canDm = p ? p.can_dm : true;
  const ready = !!S.ident && canDm;
  $('dmInput').disabled = !ready; $('dmSendBtn').disabled = !ready;
  $('dmInput').placeholder = !S.ident ? t('app.dm.ph_locked')
    : canDm ? t('app.dm.ph_ready')
    : (blocked ? t('app.dm.ph_blocked') : t('app.dm.ph_friends_only'));
  const warn=$('dmWarn');
  if(!S.ident){ warn.classList.remove('hidden'); warn.textContent=t('app.dm.warn_no_keys'); }
  else if(!canDm){ warn.classList.remove('hidden'); warn.textContent = blocked ? t('app.dm.warn_blocked') : t('app.dm.warn_friends_only',{name:S.peer}); }
  else warn.classList.add('hidden');
}

function renderRelPanel(){
  const p = (S.social?.partners||[]).find(x=>x.handle===S.peer);
  const isFriend = p?p.is_friend:false, isBlocked=p?p.is_blocked:false;
  const el=$('relPanel');
  if(!S.peer){ el.innerHTML=''; return; }
  let html = `<h2 class="section">${esc(S.peer)}</h2><div class="card">`;
  if(isBlocked){ html += `<div class="sub">${t('app.rel.blocked_note')}</div>`; }
  else {
    if(isFriend) html += `<div class="row" style="margin-bottom:.6rem"><span class="pill">${t('app.rel.friend')}</span></div>`;
    else html += `<button class="ghost full iconrow" style="margin-bottom:.6rem" data-act="requestFriend" data-handle="${esc(S.peer)}">${icon('people',16)} ${t('app.rel.add_friend')}</button>`;
    html += `<button class="danger full iconrow" data-act="blockUser" data-handle="${esc(S.peer)}">${icon('ban',16)} ${t('app.rel.block_perm')}</button>`;
    html += `<div class="sub" style="margin-top:.5rem">${t('app.rel.block_hint')}</div>`;
  }
  html += `</div>`;
  el.innerHTML = html;
}

const sendDm = guard(async () => {
  const input=$('dmInput'); const body=input.value.trim(); if(!body||!S.peer) return;
  if(!S.ident){ toast(t('app.dm.unlock'),'err'); return; }
  await crypto_();
  // Seal the body to the recipient (so they can read it) and to ourselves (so we
  // can re-read our own sent message). The server only ever sees ciphertext.
  let their; try { their = (await api('/api/keys/'+enc(S.peer))).public_key; }
    catch { toast(t('app.dm.no_keys',{name:S.peer}),'err'); return; }
  const ph = textToHex(body);
  const sealed_for_recipient = E.seal(their, ph);
  const sealed_for_sender = E.seal(S.ident.public, ph);
  await api(`/api/social/${enc(S.me)}/dm/${enc(S.peer)}`,'POST',{sealed_for_recipient, sealed_for_sender});
  input.value=''; loadConversation(); loadSocial();
});
const requestFriend = guard(async h => { await api(`/api/social/${enc(S.me)}/friend/${enc(h)}`,'POST'); toast(t('app.toast.friend_sent',{name:h}),'ok'); loadSocial(); });
const acceptFriend = guard(async h => { await api(`/api/social/${enc(S.me)}/accept/${enc(h)}`,'POST'); toast(t('app.toast.now_friends',{name:h}),'ok'); loadSocial(); if(S.peer===h) loadConversation(); });
const blockUser = guard(async h => {
  const v = await modal({title:t('app.block.title',{name:h}), message:t('app.block.message'), submitText:t('app.btn.block')});
  if(v===null) return;
  await api(`/api/social/${enc(S.me)}/block/${enc(h)}`,'POST'); toast(t('app.toast.blocked',{name:h})); loadSocial(); if(S.peer===h) loadConversation();
});
const setPolicy = guard(async isFriendsOnly => {
  await api(`/api/social/${enc(S.me)}/policy`,'POST',{is_friends_only:isFriendsOnly});
  toast(isFriendsOnly?t('app.toast.dms_friends'):t('app.toast.dms_open'),'ok');
});

/* ── Realtime ───────────────────────────────────────────────────── */
function connectWS(){
  const proto = location.protocol==='https:'?'wss':'ws';
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  ws.onopen = () => {
    $('online').textContent = t('app.ws.live');
    // Catch up on anything that changed while we were disconnected.
    if (S.mode==='chat' && S.server && S.channel) loadMessages();
    if (S.mode==='dms' && S.peer) loadConversation();
  };
  ws.onmessage = ev => {
    let m; try { m = JSON.parse(ev.data); } catch { return; }
    if (m.type==='presence') { $('online').textContent = t('app.ws.online',{n:m.online}); return; }
    if ((m.type==='message'||m.type==='reaction') && m.server===S.server && m.channel===S.channel) loadMessages();
    if (m.type==='server') { loadServers(); loadMe(); loadProposals(); }
    if (m.type==='channel' && m.server===S.server) selectServer(S.server);
    if (m.type==='proposal') { loadProposals(); if(!m.server||m.server===S.server) selectServer(S.server); }
    if (m.type==='emoji' && m.server===S.server) { loadEmojis(); loadMessages(); }
    if (m.type==='clock') loadMe();
    if (m.type==='social' && (m.a===S.me || m.b===S.me)) {
      loadSocial();
      if(S.peer && (m.a===S.peer || m.b===S.peer)) loadConversation();
    }
  };
  ws.onclose = () => { $('online').textContent = t('app.ws.reconnecting'); setTimeout(connectWS, 1500); };
  ws.onerror = () => { try { ws.close(); } catch {} };
}

function esc(s){ return (s+'').replace(/[&<>"']/g, c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
['handleInput','emailInput','passwordInput'].forEach(id=>$(id)?.addEventListener('keydown', e=>{ if(e.key==='Enter') submitAuth(); }));
$('dmInput').addEventListener('keydown', e=>{ if(e.key==='Enter') sendDm(); });

/* ── Event delegation ───────────────────────────────────────────────
   The CSP forbids inline handlers (`script-src 'self'`, no 'unsafe-inline'),
   so every clickable declares a `data-act` (and any args as `data-*`) and a
   single delegated listener dispatches through this registry. Handlers added
   directly in JS (element.onclick = …) are unaffected by the CSP and stay. */
const ACTIONS = {
  submitAuth:     () => submitAuth(),
  toggleAuthMode: () => toggleAuthMode(),
  resendVerify:   () => resendVerify(),
  setModeChat:    () => setMode('chat'),
  setModeDms:     () => showFriends(),
  showFriends:    () => showFriends(),
  friendTab:   el => setFriendTab(el.dataset.tab),
  sendFriendRequest: () => sendFriendRequest(),
  openDmBtn:   el => openDm(el.dataset.handle),
  openSettings:   () => openSettings(),
  toggleNav:      () => document.body.classList.toggle('nav-open'),
  closeNav:       () => document.body.classList.remove('nav-open'),
  toggleTheme:    () => toggleTheme(),
  logout:         () => logout(),
  newServer:      () => newServer(),
  browseServers:  () => browseServers(),
  joinByCode:     () => joinByCode(),
  joinPublic:  el => joinPublic(el.dataset.slug),
  inviteToServer: () => inviteToServer(),
  openServerSettings: () => openServerSettings(),
  newChannel:     () => newChannel(),
  sendMessage:    () => sendMessage(),
  attachFile:     () => $('attachInput').click(),
  removeAtt:   el => { const i=+el.dataset.i, p=PENDING[i]; if(p){ if(p.url) URL.revokeObjectURL(p.url); PENDING.splice(i,1); renderPending(); } },
  toggleAttSpoiler: el => { const p=PENDING[+el.dataset.i]; if(p){ p.is_spoiler=!p.is_spoiler; renderPending(); } },
  revealSpoiler: el => el.classList.add('revealed'),
  revealMedia:   el => el.classList.add('revealed'),
  openMedia:   el => window.open(el.dataset.url, '_blank', 'noopener'),
  newProposal:    () => newProposal(),
  advance15:      () => advance(15),
  startDm:        () => startDm(),
  sendDm:         () => sendDm(),
  join:           () => join(),
  becomeCitizen:  () => becomeCitizen(),
  devEndorse:     () => devEndorse(),
  cancelReply:    () => { S.reply = null; setReplyBar(); },
  react:       el => react(+el.dataset.id, el.dataset.emoji),
  jumpTo:      el => jumpTo(+el.dataset.id),
  setReply:    el => setReply(+el.dataset.id, el.dataset.who),
  quickReact:  el => quickReact(+el.dataset.id),
  vote:        el => vote(+el.dataset.id, el.dataset.aye==='1'),
  addEmoji:       () => addEmoji(),
  voteEmoji:   el => voteEmoji(+el.dataset.id, el.dataset.up==='1'),
  encryptChannel: () => encryptChannel(),
  shareKeys:      () => shareKeys(),
  acceptFriend:  el => acceptFriend(el.dataset.handle),
  requestFriend: el => requestFriend(el.dataset.handle),
  blockUser:     el => blockUser(el.dataset.handle),
};
const CHANGES = {
  setPolicy: el => setPolicy(el.checked),
};
document.addEventListener('click', ev => {
  const el = ev.target.closest('[data-act]'); if(!el) return;
  const fn = ACTIONS[el.dataset.act]; if(!fn) return;
  if(el.tagName==='A') ev.preventDefault(); // anchors used as buttons
  fn(el, ev);
});
document.addEventListener('change', ev => {
  const el = ev.target.closest('[data-change]'); if(!el) return;
  const fn = CHANGES[el.dataset.change]; if(fn) fn(el, ev);
});
boot();
