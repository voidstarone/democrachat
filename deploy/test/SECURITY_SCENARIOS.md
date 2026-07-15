# Security scenario matrix

Each scenario is asserted by `deploy/test/posture.sh` (container-free; builds and
runs the binary on loopback). IDs are referenced from the script output and from
`docs/security-audit.md`.

| ID  | Scenario | Expected |
|-----|----------|----------|
| S1  | Security headers on `/` | CSP (incl. `script-src 'self'` with **no** `'unsafe-inline'`, `object-src 'none'`, `frame-ancestors 'none'`, `form-action 'self'`), `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy`, HSTS, `Permissions-Policy` all present; `/app.js` served as `text/javascript` |
| S2  | Mutation with no session | `401` — actor comes from the session, never the body |
| S3  | Mutation with a `csrf` cookie but no `X-CSRF-Token` header | `403` (double-submit CSRF) |
| S4  | Registration & login | valid 16+ char password → `200` + `sid` cookie; short password → `400`; wrong password → opaque `401` |
| S5  | DM IDOR — one user reads another's `/api/social/:me` | `403` (you may only act as yourself) |
| S6  | Impersonation — a `handle` field smuggled in a post body | ignored; message author is the **session** user |
| S7  | Oversized request body (70 KB) | `413` (64 KB `DefaultBodyLimit`) |
| S8  | Burst of login attempts | eventually `429` + `Retry-After` (Auth bucket, 10/60s, per peer IP) |
| S9  | WebSocket upgrade | cross-origin → rejected; no session → rejected; same-origin + session → connects |
| S10 | Boot with a placeholder/short session secret on a **public** bind | process exits non-zero (fail-closed) |

## Manual / design-reviewed (not scripted)

- **Password storage** — Argon2id PHC strings, per-password salt, never the
  plaintext (unit-tested in `crates/democrachat/tests/auth.rs`).
- **Session integrity** — HMAC-SHA256 over `uid`+`expiry`; a tampered uid or
  extended expiry fails constant-time verify (`session_signer.rs` tests).
- **Enumeration timing** — the account-miss login path spends one Argon2 verify on
  a dummy hash (`spend_verify_time`).
- **XSS** — all user content is HTML-escaped client-side (`esc`); CSP is the
  backstop, and it is now airtight: `script-src 'self'` with no `'unsafe-inline'`
  (JS is served from `/app.js`, handlers are event-delegated), so even a missed
  escape can inject inert markup but never executing script (audit `WEB-CSP-1`).
