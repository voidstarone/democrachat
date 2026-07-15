# Deploying democrachat

Container-based deployment mirroring the democratos sibling. Two topologies:

| Topology | Use it when | Files |
| -------- | ----------- | ----- |
| **Single box** | One machine runs everything (the common case). | repo-root [`docker-compose.yml`](../docker-compose.yml) |
| **Split (two hosts)** | You want the public proxy and the app on separate machines — proxy in a DMZ, app on an internal host. | [`prod/`](prod/) — one compose per host |

> **Why only two hosts, not a federation?** The democratos sibling shards
> communities across many app nodes (its own Postgres + etcd + shared media per
> node). democrachat is deliberately a *single process* with an in-memory model
> persisted to one JSON snapshot — there is no sharding or replication to
> distribute. So "in parts" here means splitting the **tiers** (stateless edge ↔
> stateful app) across hosts, run and upgraded independently — not running two
> app nodes. Exactly one app host owns the data.

## Trust model

Two-tier in both topologies: the **Caddy reverse proxy** is the only service that
publishes ports (80/443) and terminates TLS. The **app** listens on `:3000` and is
never internet-exposed — single-box keeps it on an internal Docker network;
split binds it to a private LAN address the edge host reaches. All public traffic
arrives through Caddy.

## Split across two hosts

See [`prod/README.md`](prod/README.md). In short: run
[`prod/app/`](prod/app/docker-compose.yml) on the internal host (owns the `/data`
volume, binds a LAN address) and [`prod/edge/`](prod/edge/docker-compose.yml) on
the public host (Caddy → the app host's LAN address). Both use `${VAR:?}` gates
and the same app-layer controls as the single box.

## Single box

## First run

```sh
cp .env.example .env
# Edit .env:
#   DEMOCRACHAT_SESSION_SECRET=$(openssl rand -hex 32)
#   SITE_ADDRESS=chat.example.com   # a real DNS name pointed at this host
docker compose up -d --build
```

Caddy will obtain a Let's Encrypt certificate for `SITE_ADDRESS` automatically
(ports 80 and 443 must be reachable from the internet for the ACME challenge).

## What's hardened

- **Fail-closed secrets** — every secret in `docker-compose.yml` is `${VAR:?}`, so
  the stack will not start with one missing. The app *additionally* refuses to
  boot on a public bind with a placeholder/short `DEMOCRACHAT_SESSION_SECRET`.
- **Non-root, read-only container** — the app runs as the unprivileged `app` user
  on a `read_only` root filesystem, with `/tmp` on tmpfs and durable state in a
  named volume. `cap_drop: [ALL]` and `no-new-privileges` remove kernel-level
  escalation paths.
- **Resource guards** — `mem_limit`, `cpus`, `pids_limit` bound blast radius.
- **TLS everywhere public** — Caddy auto-provisions certs; the app sets
  `Secure` cookies + HSTS behind it.
- **App-layer controls** (see `docs/security-audit.md`) — Argon2id passwords,
  HMAC session cookies, CSRF double-submit, per-IP rate limiting, security
  headers/CSP, WebSocket origin checks.

## Production checklist

- [ ] `DEMOCRACHAT_SESSION_SECRET` is a fresh 32-byte random value, not the
      placeholder.
- [ ] `SITE_ADDRESS` DNS A/AAAA record points at this host; 80/443 open.
- [ ] Back up the `data` volume (holds `democrachat.json`, the whole dataset).
- [x] Base-image digests pinned in `Dockerfile` + compose (`@sha256:…`) for a
      reproducible supply chain — refresh with the snippet in the `Dockerfile` header.
- [ ] `cargo deny check` runs green in CI (see `deny.toml`).
- [ ] Run `deploy/test/posture.sh` against the deployed URL.
