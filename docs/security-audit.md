# democrachat security audit & threat model

_Last updated: 2026-07-13. Controls ported from the democratos sibling and adapted
to democrachat's JS-first SPA + JSON-API + WebSocket architecture._

This document is the standing threat model: the controls in place, the findings
that were fixed, and the items still open. IDs are stable so they can be
referenced from code review and from `deploy/test/SECURITY_SCENARIOS.md`.

## Trust model

democrachat is a self-governing chat platform. The security-critical invariants:

1. **Identity is the session cookie, never client-asserted.** Every mutation
   derives its actor from a signed `sid` cookie. No handle in a request body is
   ever trusted for authorization.
2. **The franchise is criteria-only** (a domain invariant, not a web control):
   there is no API, role, or admin action that grants citizenship.
3. **DMs, blocks, friendships, and social state are private to their owner.**

## Controls in place

### Authentication & sessions (CONTROL-AUTH)
- **Argon2id** password hashing, per-password random salt, PHC-string storage;
  plaintext never persisted. Length policy 16–256 (a high floor beats composition
  rules; the ceiling caps an Argon2 hashing-DoS). `crates/app/src/auth/`,
  `crates/domain/src/credentials/`.
- **HMAC-SHA256 signed session cookie** `sid = uid.expires.mac`; the MAC covers
  both uid and expiry, so neither can be forged or extended. Constant-time verify;
  expiry checked against the server clock (a replayed expired cookie is rejected).
  `HttpOnly; SameSite=Lax; Max-Age=30d`, `Secure` behind TLS.
  `crates/app/src/session/session_signer.rs`, `crates/adapter-web/src/auth.rs`.
- **Enumeration-timing defense** — the account-miss / passwordless path spends one
  Argon2 verify on a cached dummy hash so it costs the same as a real check;
  login failures collapse to one opaque `401`. `auth/spend_verify_time.rs`.

### Authorization (CONTROL-AUTHZ)
- Every mutating endpoint calls `require_actor` (401 if unauthenticated) and uses
  that identity — closing impersonation.
- Social endpoints enforce `require_self` (`:me` in the path must equal the
  session actor) — closing DM IDOR.

### Web hardening (CONTROL-WEB)
- **Security headers** on every response: CSP (`default-src 'self'`,
  `script-src 'self'` — no `'unsafe-inline'`, `object-src 'none'`,
  `base-uri 'self'`, `frame-ancestors 'none'`, `form-action 'self'`,
  `connect-src 'self'`), `X-Frame-Options: DENY`, `nosniff`,
  `Referrer-Policy: same-origin`, HSTS, `Permissions-Policy`.
  `middleware/security_headers.rs`.
- **No inline script** — the client's JS is served from same-origin `/app.js`
  and every handler is attached by event delegation (`data-act` dispatch), so
  `script-src 'self'` holds with no escape hatch: an injected inline `<script>`
  or `onclick=` is inert, as is `eval`. `src/app.js`, `src/index.html`.
- **CSRF** double-submit: `X-CSRF-Token` header must equal the client-set `csrf`
  cookie (constant-time), checked on every mutation before any work.
  `middleware/csrf.rs`.
- **Rate limiting** per peer IP, fixed-window: Auth 10/60s, Write 120/60s, `429` +
  `Retry-After`. Keyed on the direct connection peer, never `X-Forwarded-For`.
  `middleware/rate_limit.rs`.
- **Body-size limit** 64 KB (`DefaultBodyLimit`) — no uploads, so a tight cap.
- **WebSocket** upgrade requires a valid session **and** a same-origin `Origin`
  (CSWSH defense). `ws.rs`.
- **Output escaping** — all user content HTML-escaped client-side (`esc`); CSP is
  the backstop. `Cache-Control: no-store` on the SPA shell.

### Secrets & deployment (CONTROL-DEPLOY)
- **Fail-closed session secret** — on a non-loopback bind the app exits if
  `DEMOCRACHAT_SESSION_SECRET` is absent, a `CHANGE_ME` placeholder, or <16 chars
  (a weak secret makes cookies forgeable). Loopback falls back to a random
  per-process key with a warning. `crates/democrachat/src/main.rs`.
- **Container** — non-root `app` user, `--locked` release build, base images
  pinned by `@sha256:` index digest (multi-arch), read-only rootfs,
  `cap_drop: [ALL]`, `no-new-privileges`, tmpfs `/tmp`, resource limits.
  `Dockerfile`, `docker-compose.yml`.
- **Edge/proxy** — Caddy is the only published service; TLS via Let's Encrypt; the
  app's `:3737` is never published. `${VAR:?}` secret gates so the stack won't
  start with a missing secret. `deploy/`.
- **Topologies** — single-box (`docker-compose.yml`) or a two-host tier split
  (stateless edge ↔ stateful app, `deploy/prod/`). In both, only the proxy is
  internet-exposed; the split's edge→app hop must stay on a trusted/tunnelled
  private network (documented in `deploy/prod/README.md`). democrachat is a
  single in-memory node — no sharding/replication, so no multi-node federation.
- **Supply chain** — `deny.toml` (advisories/bans/sources) via `cargo deny check`.

## Findings

| ID | Sev | Finding | Status |
|----|-----|---------|--------|
| P0-IMPERSONATION | Critical | Every mutation trusted a client-supplied actor handle → act as anyone | **FIXED** — session-derived actor (`require_actor`) |
| P0-DM-IDOR | High | `/api/social/:me/...` readable/writable for any `:me` → read anyone's DMs | **FIXED** — `require_self` |
| P0-NOAUTH | Critical | No passwords/sessions at all | **FIXED** — Argon2 + HMAC sessions |
| WEB-CSRF | High | No CSRF protection on JSON mutations | **FIXED** — double-submit `X-CSRF-Token` |
| WEB-HEADERS | Medium | No security headers/CSP | **FIXED** — headers middleware |
| WEB-RATELIMIT | Medium | No brute-force / Argon2-DoS throttle | **FIXED** — per-IP limiter |
| WEB-WS-ORIGIN | Medium | WebSocket had no auth or Origin check (CSWSH) | **FIXED** — auth + Origin check |
| DEP-SECRET | High | No fail-closed secret gate | **FIXED** — refuse boot on weak secret (public bind) |
| WEB-CSP-1 | Low | CSP kept `script-src 'unsafe-inline'` because the SPA used inline event handlers | **FIXED** — JS externalized to `/app.js`, all handlers event-delegated, `script-src 'self'` |
| DEP-2 | Low | Base-image tags not pinned to digests | **FIXED** — `@sha256:` index digests in `Dockerfile` + compose (multi-arch, refresh snippet in the `Dockerfile` header) |

## Verification

- Unit/integration: `cargo test` (session signing, password policy, auth
  use-cases, IDOR-relevant social rules).
- End-to-end posture: `deploy/test/posture.sh` — 20 checks (see
  `SECURITY_SCENARIOS.md`), all green.
