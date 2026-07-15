# Split deployment — edge host + app host

Run democrachat across **two machines**: a public **edge** host (the reverse
proxy, the only internet-exposed box) and an internal **app** host (the single
stateful node that owns the data). Each is a self-contained `docker compose`
project you bring up independently.

```
                internet
                   │  (SITE_ADDRESS A-record → edge host public IP)
            router 80/443 forward
                   ▼
        ┌──────── edge host ────────┐         ┌──────── app host ────────┐
        │ caddy :80/:443 (TLS)       │  LAN    │ democrachat  ${APP_BIND}  │
        │   reverse_proxy ───────────┼────────▶│   :3737  ── /data volume  │
        └────────────────────────────┘         └───────────────────────────┘
             stateless, replaceable                  stateful: the whole DB
```

**This is a tier split, not a federation.** democrachat is one process with an
in-memory model persisted to a single JSON snapshot; it is not sharded or
replicated. Exactly **one** app host runs. (Contrast the democratos sibling,
which shards communities across nodes with per-node Postgres + etcd + shared
media — democrachat has none of that machinery by design.) The win here is
operational: the public proxy and the app upgrade, restart, and get firewalled
independently, and only the edge box is exposed.

## Bring-up

Pick two hosts on a shared private network (LAN, VPC, or a WireGuard/tailnet).
Say the app host is `10.0.0.4` and the edge host is public.

### 1. App host (internal)

Prepare the external drive first (durable state — the DB snapshot and the media
blobs — lives on it, not the SD card). Assuming it is mounted at `/mnt/ssd`:

```sh
sudo mkdir -p /mnt/ssd/democrachat/data /mnt/ssd/democrachat/media
sudo chown -R "$(id -u):$(id -g)" /mnt/ssd/democrachat   # PUID:PGID in .env
```

Then bring the app up:

```sh
cd deploy/prod/app
cp .env.example .env && $EDITOR .env
#   DEMOCRACHAT_SESSION_SECRET=$(openssl rand -hex 32)
#   APP_BIND=192.168.1.5:3737     # this host's PRIVATE address, never 0.0.0.0
#   DATA_DIR / MEDIA_DIR          # the two dirs you just created
#   PUID / PGID                   # your `id -u` / `id -g`
docker compose up -d --build      # builds arm64 on the Pi (first build is slow)
curl -fsS http://192.168.1.5:3737/ >/dev/null && echo "app up"   # from the LAN
```

The app publishes **only** on `APP_BIND` (a host-IP-scoped port), keeps the DB
snapshot on `DATA_DIR` and all uploaded media on `MEDIA_DIR` (both on your drive),
and — because this is a non-loopback bind — refuses to boot without a real
`DEMOCRACHAT_SESSION_SECRET` (fail-closed). Every path, port, uid, and limit is an
env var in `.env`.

> **Drive must be mounted before Docker starts.** If `/mnt/ssd` is not mounted at
> boot, Docker will bind the *empty* mount point on the SD card and the app will
> silently start with no data. Give the drive a stable `/etc/fstab` entry (by
> `UUID=…`, options `defaults,nofail`) and confirm `mountpoint -q /mnt/ssd` before
> `docker compose up`. Format the drive **ext4** — exFAT/NTFS don't carry the Unix
> ownership the `user:`/bind-mount model relies on.

### 2. Edge host (public)

```sh
cd deploy/prod/edge
cp .env.example .env && $EDITOR .env
#   SITE_ADDRESS=chat.example.com
#   APP_UPSTREAM=10.0.0.4:3737     # must equal the app host's APP_BIND
docker compose up -d
```

Caddy obtains a Let's Encrypt cert for `SITE_ADDRESS` automatically (80/443 must
be reachable from the internet). Open `https://SITE_ADDRESS`.

#### Already running Caddy on the edge host?

If the edge Pi already runs Caddy (not this compose), don't run the edge project —
just add one site block to your existing `Caddyfile`, pointing at the app host's
`APP_BIND`, and reload (`caddy reload` / `systemctl reload caddy`, or
`docker exec <caddy> caddy reload --config /etc/caddy/Caddyfile`):

```caddy
chat.example.com {
	encode gzip zstd
	# The /ws WebSocket upgrade proxies transparently — no extra config needed.
	reverse_proxy 192.168.1.5:3737     # = the app host's APP_BIND
	header {
		Strict-Transport-Security "max-age=63072000; includeSubDomains"
		X-Content-Type-Options "nosniff"
		X-Frame-Options "DENY"
		Referrer-Policy "same-origin"
		-Server
	}
}
```

That block is the whole integration: everything else (TLS, media serving, the
WebSocket) is handled by the app behind it. `deploy/prod/edge/Caddyfile` is the
same block parameterised, if you'd rather copy from there.

## The LAN hop (read this)

The edge → app hop (`APP_UPSTREAM`) is **plain HTTP on your private network**.
Session cookies and messages cross it in the clear, so it must ride a trusted
path:

- Keep both hosts on a private network and **firewall `APP_BIND` so only the edge
  host can reach it** — never expose `:3737` to the internet or an untrusted LAN.
- Best: put the hop on a **WireGuard/tailnet** and set `APP_BIND` /
  `APP_UPSTREAM` to the tunnel addresses (`100.x.x.x:3737`) — then the hop is
  encrypted end to end.
- The app still sets `Secure` cookies + HSTS (the *public* origin is HTTPS at the
  edge); `DEMOCRACHAT_SECURE_COOKIES=1` is already set in the app compose.

This mirrors the democratos guidance to keep inter-host links on an isolated,
authenticated network (it terminates TLS on those links; democrachat's simpler
single-node hop leans on a trusted/tunnelled private network instead).

## Scaling & upgrades

- **More app nodes?** Not supported — the store is in-process. Scale vertically
  (bump `APP_MEM`/`APP_CPUS` in the app `.env`) and back up `DATA_DIR` + `MEDIA_DIR`.
- **Upgrade the app** without touching the edge: `docker compose up -d --build`
  in `app/` (brief blip while it restarts; the edge keeps serving once it's back).
- **Replace the edge** freely — it is stateless apart from the `caddy_data`
  volume (the ACME account + certs; keep it to avoid re-issuing).

## Checklist

- [ ] `DEMOCRACHAT_SESSION_SECRET` is a fresh 32-byte value (app `.env`).
- [ ] `APP_BIND` is a private address; `:3737` is firewalled to the edge host only.
- [ ] The external drive is ext4, in `/etc/fstab` (`nofail`), and mounted **before**
      Docker; `DATA_DIR`/`MEDIA_DIR` exist and are chowned to `PUID:PGID`.
- [ ] Edge upstream (`APP_UPSTREAM`, or the `reverse_proxy` in your existing Caddy)
      equals `APP_BIND`.
- [ ] `SITE_ADDRESS` DNS points at the edge host; 80/443 forwarded to it only.
- [ ] Back up `DATA_DIR` (the whole database) and `MEDIA_DIR` (all uploads). If
      `DEMOCRACHAT_DATA_KEK` is set, back the key up **separately**.
- [ ] `deploy/test/posture.sh` (point it at `https://SITE_ADDRESS`) is green.
