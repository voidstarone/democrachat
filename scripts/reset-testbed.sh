#!/usr/bin/env bash
#
# Reset the democrachat testbed to a known-good state and serve it.
#
# Wipes the testbed's data snapshot + media, rebuilds the binary, and boots the
# web server with the rich `--demo` seed: one server ("Testbed") carrying every
# kind of channel and content. Deterministic — every run lands in the same state,
# so it backs both manual click-through and automated tests.
#
# Usage:
#   scripts/reset-testbed.sh              # reset + run in the foreground
#   PORT=4000 scripts/reset-testbed.sh    # pick the port (default 3939)
#   scripts/reset-testbed.sh --detach     # reset + run in the background, print URL
#
# Sign in as any seeded account (ada, grace, hiro, mimi, nova, otto, pax, troll)
# with the password: democrachat-demo!
# ada is the founder; grace/hiro/mimi/nova are citizens; otto/pax are members;
# troll is banned. hiro is police; pax is muted.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTBED="$ROOT/.testbed"
PORT="${PORT:-3939}"
ADDR="127.0.0.1:$PORT"
DETACH=0
[ "${1:-}" = "--detach" ] && DETACH=1

echo "▸ stopping any testbed server on $ADDR"
pkill -f "democrachat serve.*--addr $ADDR" 2>/dev/null || true
sleep 0.5

echo "▸ wiping $TESTBED"
rm -rf "$TESTBED"
mkdir -p "$TESTBED/media"

echo "▸ building"
cargo build -q -p democrachat

# The store snapshot and media tier live under .testbed so a reset is a clean rm.
# Unset DATABASE_URL so the file store (not Postgres) backs the demo.
export DEMOCRACHAT_DATA="$TESTBED/db.json"
export DEMOCRACHAT_MEDIA_DIR="$TESTBED/media"
unset DATABASE_URL

BIN="$ROOT/target/debug/democrachat"

if [ "$DETACH" = "1" ]; then
  "$BIN" serve --demo --addr "$ADDR" > "$TESTBED/server.log" 2>&1 &
  PID=$!
  # Wait until it answers (or the process dies).
  for _ in $(seq 1 50); do
    if curl -sf -o /dev/null "http://$ADDR/"; then
      echo "▸ testbed up: http://$ADDR  (pid $PID, log $TESTBED/server.log)"
      exit 0
    fi
    kill -0 "$PID" 2>/dev/null || { echo "server exited early; see $TESTBED/server.log"; exit 1; }
    sleep 0.2
  done
  echo "server did not become ready; see $TESTBED/server.log"
  exit 1
fi

echo "▸ serving http://$ADDR  (Ctrl-C to stop)"
exec "$BIN" serve --demo --addr "$ADDR"
