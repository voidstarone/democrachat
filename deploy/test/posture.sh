#!/usr/bin/env bash
# democrachat security posture check — container-free.
#
# Builds the release binary, runs it on loopback (dev), and asserts the security
# controls from docs/security-audit.md behave. Also verifies the fail-closed boot
# gate. Exits non-zero if any check fails.
#
#   deploy/test/posture.sh
#
# Scenario IDs referenced here match deploy/test/SECURITY_SCENARIOS.md.

set -u
cd "$(dirname "$0")/../.." || exit 1

PORT=${PORT:-3999}
B="http://127.0.0.1:${PORT}"
TMP="$(mktemp -d)"
DATA="${TMP}/data.json"
PASS=0; FAIL=0
# Send the csrf token as a cookie (merges with a session jar) + matching header.
CSRF='-b csrf=tok -H X-CSRF-Token:tok'

ok()   { printf "  [ok] %s\n" "$1"; PASS=$((PASS+1)); }
bad()  { printf "  [XX] %s\n" "$1"; FAIL=$((FAIL+1)); }
# check <desc> <expected> <actual>
check(){ if [ "$2" = "$3" ]; then ok "$1 ($3)"; else bad "$1 (want $2, got $3)"; fi; }

echo "building release binary..."
cargo build --release -q -p democrachat || { echo "build failed"; exit 1; }
BIN=target/release/democrachat

echo "starting server on ${B} ..."
DEMOCRACHAT_DATA="$DATA" "$BIN" serve --dev --addr "127.0.0.1:${PORT}" >"${TMP}/log" 2>&1 &
SRV=$!
trap 'kill -9 $SRV 2>/dev/null; rm -rf "$TMP"' EXIT
# wait for readiness
for _ in $(seq 1 50); do curl -sf "$B/api/config" >/dev/null 2>&1 && break; sleep 0.2; done

code(){ curl -s -o /dev/null -w "%{http_code}" "$@"; }

echo "== S1: security headers present on / =="
H="$(curl -s -D - -o /dev/null "$B/")"
for hdr in "content-security-policy" "x-frame-options: DENY" "x-content-type-options: nosniff" "referrer-policy" "strict-transport-security" "permissions-policy"; do
  if printf '%s' "$H" | grep -iq "$hdr"; then ok "header: $hdr"; else bad "missing header: $hdr"; fi
done
if printf '%s' "$H" | grep -iq "content-security-policy.*object-src 'none'"; then ok "CSP object-src none"; else bad "CSP object-src none"; fi
# WEB-CSP-1: script-src is 'self' with no inline escape hatch (handlers are delegated).
if printf '%s' "$H" | grep -iq "script-src 'self'[;[:space:]]" && ! printf '%s' "$H" | grep -iq "script-src[^;]*unsafe-inline"; then
  ok "CSP script-src 'self' (no unsafe-inline)"; else bad "CSP script-src still allows inline script"; fi
check "app.js served as javascript" "text/javascript; charset=utf-8" "$(curl -s -o /dev/null -w '%{content_type}' "$B/app.js")"

echo "== S2: mutation without a session -> 401 =="
check "found server w/o auth" 401 "$(code $CSRF -X POST -H 'content-type: application/json' -d '{"name":"x"}' "$B/api/servers")"

echo "== S3: CSRF — mutation with cookie but no token header -> 403 =="
check "no csrf header" 403 "$(code -X POST -H 'content-type: application/json' -H 'Cookie: csrf=abc' -d '{}' "$B/api/register")"

echo "== S4: register + login establish a session =="
J="${TMP}/jar"
reg="$(code -c "$J" $CSRF -X POST -H 'content-type: application/json' -d '{"handle":"alice","password":"correct horse!!!"}' "$B/api/register")"
check "register (valid pw)" 200 "$reg"
check "register (short pw) -> 400" 400 "$(code $CSRF -X POST -H 'content-type: application/json' -d '{"handle":"eve","password":"short"}' "$B/api/register")"
check "login wrong pw -> 401" 401 "$(code $CSRF -X POST -H 'content-type: application/json' -d '{"handle":"alice","password":"nope nope nope!!"}' "$B/api/login")"

echo "== S5: DM IDOR — reading another user's social state -> 403 =="
code -c "${TMP}/bob" $CSRF -X POST -H 'content-type: application/json' -d '{"handle":"bob","password":"correct horse!!!"}' "$B/api/register" >/dev/null
check "bob reads /social/alice" 403 "$(code -b "${TMP}/bob" "$B/api/social/alice")"

echo "== S6: impersonation — body 'handle' field is ignored =="
code -b "$J" $CSRF -X POST -H 'content-type: application/json' -d '{"name":"AliceLand"}' "$B/api/servers" >/dev/null
code -b "$J" $CSRF -X POST -H 'content-type: application/json' -d '{"name":"general"}' "$B/api/servers/aliceland/channels" >/dev/null
code -b "$J" $CSRF -X POST -H 'content-type: application/json' -d '{"body":"hi","handle":"bob"}' "$B/api/servers/aliceland/channels/general/messages" >/dev/null
author="$(curl -s -b "$J" "$B/api/servers/aliceland/channels/general/messages" | python3 -c 'import sys,json;print(json.load(sys.stdin)[0]["author"])' 2>/dev/null)"
check "message author is the session user, not the body field" "alice" "$author"

echo "== S7: oversized body -> 413 =="
python3 -c "print('{\"handle\":\"z\",\"password\":\"'+'a'*70000+'\"}')" > "${TMP}/big"
check "70KB body" 413 "$(code $CSRF -X POST -H 'content-type: application/json' --data-binary @"${TMP}/big" "$B/api/register")"

echo "== S8: auth rate limit (Auth bucket ~10/60s) fires 429 =="
seen429=0
for _ in $(seq 1 16); do
  c="$(code $CSRF -X POST -H 'content-type: application/json' -d '{"handle":"nobody","password":"correct horse!!!"}' "$B/api/login")"
  [ "$c" = "429" ] && seen429=1
done
if [ "$seen429" = "1" ]; then ok "login burst eventually 429s"; else bad "no 429 under login burst"; fi

echo "== S9: WebSocket rejects a cross-origin upgrade =="
SID="$(awk '/sid/{print $7}' "$J" | tail -1)"
wsres="$(python3 - "$SID" "$PORT" <<'PY'
import asyncio,sys
try:
    import websockets
except Exception:
    print("skip"); sys.exit(0)
sid,port=sys.argv[1],sys.argv[2]
async def check(hdrs):
    try:
        async with websockets.connect(f"ws://127.0.0.1:{port}/ws", additional_headers=hdrs) as ws:
            await asyncio.wait_for(ws.recv(),timeout=2); return "connected"
    except Exception: return "rejected"
async def main():
    bad=await check({"Cookie":f"sid={sid}","Origin":"http://evil.example"})
    good=await check({"Cookie":f"sid={sid}","Origin":f"http://127.0.0.1:{port}","Host":f"127.0.0.1:{port}"})
    none=await check({})
    print("bad-origin",bad,"| good-origin",good,"| no-cookie",none)
asyncio.run(main())
PY
)"
echo "  ws: $wsres"
case "$wsres" in
  skip) ok "ws (skipped — websockets not installed)";;
  *"bad-origin rejected"*"good-origin connected"*"no-cookie rejected"*) ok "ws origin+auth enforced";;
  *) bad "ws origin/auth check";;
esac

kill -9 $SRV 2>/dev/null

echo "== S10: fail-closed boot — public bind + placeholder secret exits non-zero =="
DEMOCRACHAT_SESSION_SECRET="CHANGE_ME_please" DEMOCRACHAT_DATA="${TMP}/d2.json" \
  "$BIN" serve --addr "0.0.0.0:$((PORT+1))" >"${TMP}/boot" 2>&1 &
BP=$!; sleep 1
if kill -0 $BP 2>/dev/null; then bad "server should have refused to boot"; kill -9 $BP 2>/dev/null; else ok "refused to boot with placeholder secret on public bind"; fi

echo
echo "posture: ${PASS} passed, ${FAIL} failed"
[ "$FAIL" -eq 0 ]
