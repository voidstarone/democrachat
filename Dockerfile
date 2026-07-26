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
#   docker pull rust:1.97-bookworm
#   docker image inspect rust:1.97-bookworm --format '{{index .RepoDigests 0}}'
# and paste the new digest below (keep the human-readable tag alongside it).

# ---- builder ----
# Rust 1.97: the locked dependency graph now includes crates that require the
# 2024 edition (base64ct, clap 4.6, …), which needs a toolchain newer than the
# former 1.83 pin.
FROM rust:1.97-bookworm@sha256:77fac8b98f9f46062bb680b6d25d5bcaabfc400143952ebc572e924bcbedc3fa AS builder
# protobuf-compiler: the etcd control-plane adapter (etcd-client) compiles .proto
# files in its build script and needs `protoc` present at build time.
RUN apt-get update \
 && apt-get install -y --no-install-recommends protobuf-compiler \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# --locked: fail if Cargo.lock is stale, so the built graph is exactly the audited one.
RUN cargo build --release --locked -p democrachat

# ---- runtime ----
FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df AS runtime
# Minimal runtime deps: TLS roots + curl for the healthcheck.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# Non-root system user; durable state lives in /data owned by it.
RUN useradd --system --create-home --home-dir /home/app app \
 && mkdir -p /data \
 && chown -R app:app /data

COPY --from=builder /src/target/release/democrachat /usr/local/bin/democrachat

ENV DEMOCRACHAT_DATA=/data/democrachat.json
EXPOSE 3000
VOLUME ["/data"]

# Behind the reverse proxy, bind all interfaces on 3000. A non-loopback bind makes
# the app *require* a real DEMOCRACHAT_SESSION_SECRET (fail-closed) — see the
# composition root.
USER app
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -fsS http://127.0.0.1:3000/ || exit 1
CMD ["democrachat", "serve", "--addr", "0.0.0.0:3000"]
