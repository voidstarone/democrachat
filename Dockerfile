# democrachat — multi-stage container image.
#
# Builder compiles a release binary with a locked dependency graph; the runtime
# is a minimal Debian slim carrying only the binary + CA certs + curl (for the
# healthcheck), running as a non-root user. The compose file adds the kernel-level
# hardening (cap_drop, read-only rootfs, no-new-privileges).
#
# Base images are pinned by @sha256:<index-digest> (DEP-2), so a rebuild always
# resolves the exact same bytes even if the tag is re-pushed. The digests are the
# multi-arch manifest-list digests, so amd64 and arm64 (Raspberry Pi) both build.
# To refresh after an intentional base bump:
#   docker pull rust:1.83-bookworm
#   docker image inspect rust:1.83-bookworm --format '{{index .RepoDigests 0}}'
# and paste the new digest below (keep the human-readable tag alongside it).

# ---- builder ----
# Debian *Trixie* (not Bookworm): the HEIC/HEIF→JPEG transcoder (adapter-image)
# links libheif via libheif-sys 4.x, which needs libheif ≥ 1.19. Bookworm apt
# ships only 1.15; Trixie ships 1.19.8, matching the pinned libheif-sys.
# Rust 1.97: the locked dependency graph includes crates that require the 2024
# edition (base64ct, clap 4.6, …), which needs a toolchain newer than 1.83.
FROM rust:1.97-trixie@sha256:1bcff4befb740599103a2c7cb51058e14479b2e35e3a34a3f0dc4ede09927488 AS builder
WORKDIR /src
# libheif headers (+ pkg-config) to link the transcoder at build time; protobuf
# compiler for etcd-client's build script (it compiles .proto files with `protoc`).
RUN apt-get update \
 && apt-get install -y --no-install-recommends libheif-dev pkg-config protobuf-compiler \
 && rm -rf /var/lib/apt/lists/*
COPY . .
# --locked: fail if Cargo.lock is stale, so the built graph is exactly the audited one.
RUN cargo build --release --locked -p democrachat

# ---- runtime ----
FROM debian:trixie-slim AS runtime
# Runtime deps: TLS roots + curl for the healthcheck, and libheif (the shared lib
# the binary links for HEIC/HEIF decoding; it pulls in libde265 automatically).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl libheif1 \
 && rm -rf /var/lib/apt/lists/*

# Non-root system user; durable state lives in /data owned by it.
RUN useradd --system --create-home --home-dir /home/app app \
 && mkdir -p /data \
 && chown -R app:app /data

COPY --from=builder /src/target/release/democrachat /usr/local/bin/democrachat

ENV DEMOCRACHAT_DATA=/data/democrachat.json
EXPOSE 3737
VOLUME ["/data"]

# Behind the reverse proxy, bind all interfaces on 3737. A non-loopback bind makes
# the app *require* a real DEMOCRACHAT_SESSION_SECRET (fail-closed) — see the
# composition root.
USER app
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -fsS http://127.0.0.1:3737/ || exit 1
CMD ["democrachat", "serve", "--addr", "0.0.0.0:3737"]
