# democrachat voice channels — architecture & plan

Status: **V1 + V2 built** (2026-07-15). V3–V6 remain (see the milestone table).
Prerequisites done: Postgres port complete, tags complete.

Decision: **small rooms, full-mesh peer-to-peer WebRTC.** SFU stays a later option
(V6), not designed for up front.

**Amendment (2026-07-15):** a voice channel also carries the ordinary **text message
stream** — `ChannelKind::Voice` is a *superset* of `Text`, not a replacement. It adds
the audio roster on top; messages, reactions, encryption, etc. all still apply.

## What exists to build on

- `domain::Channel` entity — already carries `is_encrypted` and `visibility`.
- One WebSocket gateway (`crates/adapter-web/src/ws.rs`) — today **broadcast-only**:
  it fans server events out to clients, and inbound frames are ignored.
- Server-blind E2EE ethos (see `docs/federation.md` §5): the server is a ciphertext
  store + blind relay, never reads private media.
- Police mute powers (`crates/app/src/mute_service.rs`).
- Federation: shard servers + home users (see `docs/federation.md`).
- Postgres is the source of truth, with a separate pattern for ephemeral runtime
  state.

## Core decisions

1. **Model.** New `ChannelKind::{ Text, Voice }` on `domain::Channel`
   (`#[serde(default)]` = `Text`, so old snapshots keep loading — same approach as
   the `tags` field). A voice channel has **no message stream**; it has a live
   **participant roster** instead.

2. **Transport = WebRTC, topology = full-mesh P2P.** Each participant opens a direct
   `RTCPeerConnection` to every other participant. Media is browser-to-browser, so
   **no node ever sees the audio** — end-to-end encrypted by DTLS-SRTP, matching the
   server-blind design. Cost: N participants ⇒ N² connections and O(N) uplink per
   client, so rooms are capped at roughly 8. An SFU is deferred (large infra, and it
   needs insertable-streams to preserve E2EE).

3. **Signaling.** Extend the WebSocket from broadcast-only to **targeted
   bidirectional**: the server relays SDP offer/answer and ICE candidates between the
   members of a voice channel's roster. The server sees signaling metadata only,
   never media. This needs a dedicated signaling message type plus rate limiting.

4. **Roster / presence = ephemeral.** Who is currently connected lives in per-node
   in-memory state, is broadcast over the WebSocket, and is reconciled on reconnect.
   It is **not** persisted to Postgres — it is live state, not source-of-truth data.

5. **ICE / NAT traversal.** A STUN server is required (self-hosted or public). A TURN
   relay is needed for symmetric-NAT fallback; it forwards encrypted media and cannot
   decrypt it. TURN is an operational cost to document.

6. **Moderation.** Creating a voice channel uses the existing text-channel path
   (founder provisioning in Seed, or a ballot afterwards). Joining is gated by channel
   visibility + membership. **Police mute in a mesh is cooperative** — peers drop the
   muted participant's stream; there is no hard server-side enforcement without an
   SFU. This is a known limitation.

7. **Federation.** Media stays browser-to-browser regardless of which node homes each
   participant. The hard part is roster + signaling relay across home nodes. That is
   its own milestone (V5), deferred.

## Milestones

| Milestone | Scope |
|-----------|-------|
| **V1** ✅ | Domain (`ChannelKind`, voice-channel create/list). WebSocket → targeted bidirectional signaling relay (`adapter-web/src/signal.rs` `SignalHub`). In-memory roster + join/leave/relay events, per-connection rate limit, mesh cap. No media. |
| **V2** ✅ | Browser mesh (`app.js` voice module): `getUserMedia`, one `RTCPeerConnection` per peer, single-offerer negotiation, audio in/out, self mute/deafen, roster UI, WebAudio speaking indicator, STUN/TURN config via `DEMOCRACHAT_ICE_SERVERS` + `/api/config`. |
| **V3** | Ops hardening: TURN integration (env knob shipped in V2 — test/document it), room-size backpressure, reconnection resilience, device picker, richer mic-permission UX. |
| **V4** | Moderation: police force-mute, join gating by visibility, per-channel speak permission. |
| **V5** | Federation: cross-node roster + inter-node signaling relay; presence propagation (media still P2P). |
| **V6** (later, optional) | SFU for large rooms + E2EE via insertable streams; video / screenshare. |

## Verification

- **Unit** — `SignalHub` roster/relay/cap tests in `crates/adapter-web/src/signal.rs`.
- **Integration** — `crates/democrachat/tests/voice_channels.rs`: kind tagging, voice
  channels carry the text stream, text is the default.
- **End-to-end (cross-browser)** — `e2e/voice-mesh.mjs`, wired as
  `crates/democrachat/tests/voice_e2e.rs`. Boots the real server and drives **Chrome +
  Firefox** (fake media) into one voice channel, asserting the WebRTC mesh reaches
  `connected` with a live remote audio track each way. Gated behind `DEMOCRACHAT_E2E=1`
  (needs Node + `puppeteer-core` + both browsers); a no-op otherwise. Run:
  `DEMOCRACHAT_E2E=1 cargo test -p democrachat --test voice_e2e`.

> **Gotcha fixed:** the app-wide `Permissions-Policy` denied `microphone=()` outright,
> which blocks `getUserMedia` in every browser. Voice requires `microphone=(self)`
> (`crates/adapter-web/src/middleware/security_headers.rs`). Camera stays denied until
> video (V6).

## Top risks

- **Mesh scaling** — a hard cap around 8 participants; larger rooms *require* an SFU
  (V6).
- **Mute enforcement** — cooperative only in a mesh; real enforcement needs an SFU.
- **TURN** — operational cost, and the question of who runs it per federated node.
- **Signaling load** on the single WebSocket — needs a dedicated message type and
  rate limiting.
