#!/usr/bin/env bash
# Live 2-node federation harness.
#
# Brings up a real etcd (Docker) plus TWO local democrachat nodes wired to it, then
# drives cross-node scenarios end to end and asserts the outcome on BOTH nodes:
#   • user rows replicate over the signed feed (node-scoped authorization)
#   • cross-home DM command FORWARDING (a write to a scope this node doesn't own is
#     signed and forwarded to the owner)
#   • friend request + accept as a 2-home synchronous commit
#   • permanent block as a 2-home synchronous commit (the safety-critical one)
#
# It uses the debug binary + a Docker etcd (fast to iterate); docker-compose.yml in
# this dir is the fully-containerized equivalent for CI. Re-runnable: it wipes the
# etcd prefix, uses fresh data files, and frees its ports on entry and exit.
set -uo pipefail
cd "$(dirname "$0")/../.."

ETCD_IMAGE=gcr.io/etcd-development/etcd:v3.5.16
ETCD_NAME=democrachat-fedtest-etcd
ETCD_EP=http://127.0.0.1:2379
TOKEN=fedtest-cluster-secret
SECRET=fedtest_session_secret_0123456789abcdef
SEED1=1111111111111111111111111111111111111111111111111111111111111111
SEED2=2222222222222222222222222222222222222222222222222222222222222222
BIN=./target/debug/democrachat
D1=/tmp/fedtest-node1.json
D2=/tmp/fedtest-node2.json
LOG1=/tmp/fedtest-node1.log
LOG2=/tmp/fedtest-node2.log
CSRF=fedtestcsrf
JAR=/tmp/fedtest-jars
PASS=0; FAIL=0

pass(){ echo "  PASS  $1"; PASS=$((PASS+1)); }
fail(){ echo "  FAIL  $1"; FAIL=$((FAIL+1)); }

cleanup(){
  # Silence the shell's async-kill "Terminated" notices for the node subshells.
  { [ -n "${N1:-}" ] && kill "$N1" 2>/dev/null; } 2>/dev/null
  { [ -n "${N2:-}" ] && kill "$N2" 2>/dev/null; } 2>/dev/null
  wait "${N1:-}" "${N2:-}" 2>/dev/null
  docker rm -f "$ETCD_NAME" >/dev/null 2>&1
  rm -f "$D1" "$D2"; rm -rf "$JAR"
}
trap cleanup EXIT
cleanup 2>/dev/null   # clear any leftovers from a prior run
mkdir -p "$JAR"; rm -f "$D1" "$D2"

# --- HTTP helpers (CSRF double-submit + per-user cookie jar) -----------------
# $1 user  $2 method  $3 base-url  $4 path  [$5 json-body]
req(){
  local u=$1 m=$2 base=$3 path=$4 body=${5:-}
  local args=(-s -c "$JAR/$u" -b "$JAR/$u" -b "dc_csrf=$CSRF" -H "x-dc-csrf-token: $CSRF"
              -X "$m" "$base$path" -w $'\n%{http_code}')
  [ -n "$body" ] && args+=(-H 'content-type: application/json' -d "$body")
  curl "${args[@]}"
}
code(){ printf '%s' "$1" | tail -n1; }
json(){ printf '%s' "$1" | sed '$d'; }
field(){ printf '%s' "$1" | python3 -c "import sys,json;print(json.load(sys.stdin).get('$2'))" 2>/dev/null; }

echo "== 1. etcd =="
docker run -d --rm -p 2379:2379 --name "$ETCD_NAME" "$ETCD_IMAGE" \
  /usr/local/bin/etcd --advertise-client-urls "$ETCD_EP" \
  --listen-client-urls http://0.0.0.0:2379 >/dev/null || { echo "etcd failed"; exit 1; }
for i in $(seq 1 30); do docker exec "$ETCD_NAME" etcdctl endpoint health >/dev/null 2>&1 && break; sleep 0.3; done
echo "  etcd up"

echo "== 2. build =="
cargo build -q -p democrachat || exit 1

echo "== 3. launch two federated nodes =="
common_env(){
  export DEMOCRACHAT_ETCD_ENDPOINTS=$ETCD_EP
  export DEMOCRACHAT_FED_TOKEN=$TOKEN
  export DEMOCRACHAT_FED_POLL_SECS=2
  export DEMOCRACHAT_SESSION_SECRET=$SECRET
}
launch1(){ ( common_env
  export DEMOCRACHAT_NODE_ID=1 DEMOCRACHAT_NODE_SEED=$SEED1 \
         DEMOCRACHAT_FED_ADDR=127.0.0.1:4101 DEMOCRACHAT_PEERS=2=http://127.0.0.1:4102 \
         DEMOCRACHAT_DATA=$D1
  exec "$BIN" serve --addr 127.0.0.1:5101 ) >>"$LOG1" 2>&1 & N1=$!; }
launch2(){ ( common_env
  export DEMOCRACHAT_NODE_ID=2 DEMOCRACHAT_NODE_SEED=$SEED2 \
         DEMOCRACHAT_FED_ADDR=127.0.0.1:4102 DEMOCRACHAT_PEERS=1=http://127.0.0.1:4101 \
         DEMOCRACHAT_DATA=$D2
  exec "$BIN" serve --addr 127.0.0.1:5102 ) >>"$LOG2" 2>&1 & N2=$!; }
# Start each run with fresh logs (the launchers append, so node1's failover relaunch
# still accumulates onto its earlier log within a run).
: > "$LOG1"; : > "$LOG2"
launch1; launch2

A=http://127.0.0.1:5101      # node 1 web
B=http://127.0.0.1:5102      # node 2 web
FA=http://127.0.0.1:4101     # node 1 feed+command (node-only surface)
FB=http://127.0.0.1:4102     # node 2 feed+command
for i in $(seq 1 40); do
  curl -sf $A/api/config >/dev/null 2>&1 && curl -sf $B/api/config >/dev/null 2>&1 && break; sleep 0.3
done
grep -q "federation: node 1 up" "$LOG1" && echo "  node1 federated" || { echo "node1 not federated"; tail -5 "$LOG1"; exit 1; }
grep -q "federation: node 2 up" "$LOG2" && echo "  node2 federated" || { echo "node2 not federated"; tail -5 "$LOG2"; exit 1; }

echo "== 4. register users on their home nodes =="
# ana homed on node1; ben & cyd homed on node2 (ids carry the minting node's prefix).
reg(){ req "$1" POST "$2" /api/register "{\"handle\":\"$1\",\"password\":\"pw-$1-abcdefghijkl\"}"; }
[ "$(code "$(reg ana $A)")" = 200 ] && pass "register ana@node1" || fail "register ana@node1"
[ "$(code "$(reg ben $B)")" = 200 ] && pass "register ben@node2" || fail "register ben@node2"
[ "$(code "$(reg cyd $B)")" = 200 ] && pass "register cyd@node2" || fail "register cyd@node2"
# dot/eli/gus power the §15 social-graph & DM-policy tests. Register them HERE, up
# front — §13's auth-rate-limit burst saturates node2's auth bucket for 60s, so a
# registration attempted later in the run would 429. Their sessions live in the cookie
# jars, so §15 reuses them without touching the (rate-limited) auth endpoint again.
[ "$(code "$(reg dot $A)")" = 200 ] && pass "register dot@node1" || fail "register dot@node1"
[ "$(code "$(reg eli $B)")" = 200 ] && pass "register eli@node2" || fail "register eli@node2"
[ "$(code "$(reg gus $B)")" = 200 ] && pass "register gus@node2" || fail "register gus@node2"

echo "== 5. wait for cross-node user replication (signed feed) =="
# GET /api/social/<me> 404s until that node has replicated the user row.
seen(){ # $1 user $2 base
  for i in $(seq 1 30); do
    [ "$(code "$(req "$1" GET "$2" /api/social/"$1")")" = 200 ] && return 0; sleep 0.4
  done; return 1
}
seen ben $A && pass "ben replicated node2→node1" || fail "ben not seen on node1"
seen ana $B && pass "ana replicated node1→node2" || fail "ana not seen on node2"
seen cyd $A && pass "cyd replicated node2→node1" || fail "cyd not seen on node1"
# eli (node2) must reach node1 for §15's cross-home dot→eli DM to resolve.
seen eli $A && pass "eli replicated node2→node1" || fail "eli not seen on node1"

echo "== 6. cross-home DM command FORWARDING =="
# ben (home node2) sends to ana, but issued via NODE1 → node1 forwards SendDm to
# node2 (ben's home) which owns the write. 200 proves the forward + owner apply.
dm(){ req "$1" POST "$2" /api/social/"$1"/dm/"$3" '{"sealed_for_recipient":"aa","sealed_for_sender":"bb"}'; }
[ "$(code "$(dm ben $A ana)")" = 200 ] && pass "ben→ana DM forwarded node1→node2 (owner applies)" || fail "forwarded DM rejected"
# ana (home node1) → ben, issued via NODE2 → forwarded to node1.
[ "$(code "$(dm ana $B ben)")" = 200 ] && pass "ana→ben DM forwarded node2→node1 (owner applies)" || fail "forwarded DM rejected"

echo "== 7. friend request + accept as a 2-home commit =="
# ana (node1) → cyd (node2): request must land on BOTH homes.
fr(){ req "$1" POST "$2" /api/social/"$1"/friend/"$3"; }
fa(){ req "$1" POST "$2" /api/social/"$1"/accept/"$3"; }
[ "$(code "$(fr ana $A cyd)")" = 200 ] && pass "ana→cyd friend request submitted" || fail "friend request failed"
inc=$(field "$(json "$(req cyd GET $B /api/social/cyd)")" incoming_requests)
echo "$inc" | grep -q ana && pass "request reached cyd's home (node2) — incoming shows ana" || fail "request not on node2 (got: $inc)"
[ "$(code "$(fa cyd $B ana)")" = 200 ] && pass "cyd accepts ana" || fail "accept failed"
frn=$(field "$(json "$(req ana GET $A /api/social/ana)")" friends)
echo "$frn" | grep -q cyd && pass "accept committed on ana's home (node1) — friends shows cyd" || fail "accept not on node1 (got: $frn)"

echo "== 8. permanent block as a 2-home synchronous commit =="
# ana (node1) blocks ben (node2): UserHome(ana)=node1 local + UserHome(ben)=node2 forward.
[ "$(code "$(req ana POST $A /api/social/ana/block/ben)")" = 200 ] && pass "ana blocks ben submitted" || fail "block failed"
blk=$(field "$(json "$(req ana GET $A /api/social/ana)")" blocked)
echo "$blk" | grep -q ben && pass "block committed on node1 — ana.blocked shows ben" || fail "block not on node1 (got: $blk)"
# The 2-home proof: ben's DM gate runs on node2; a block that reached node2 refuses it.
ac=$(code "$(dm ben $B ana)")
[ "$ac" = 400 ] && pass "block committed on node2 — ben→ana now refused by node2's gate" || fail "block NOT on node2 (ben→ana got $ac, expected 400)"
# And node1 refuses ana→ben too.
ac=$(code "$(dm ana $A ben)")
[ "$ac" = 400 ] && pass "block enforced on node1 — ana→ben refused" || fail "ana→ben got $ac, expected 400"

# raw status-only curl (no auto cookies/CSRF) for adversarial probes.
raw(){ curl -s -o /dev/null -w '%{http_code}' "$@"; }
is(){ [ "$1" = "$2" ] && pass "$3 ($1)" || fail "$3 (got $1, want $2)"; }
isnt(){ [ "$1" != "$2" ] && pass "$3 ($1)" || fail "$3 (got $1, must not be $2)"; }
in4xx(){ case "$1" in 4*) pass "$2 ($1)";; *) fail "$2 (got $1, want 4xx)";; esac; }

echo "== 9. cross-home VOTE forwarding (the M5 governance headline) =="
# ana founds a server on node1 (founder = citizen #1) and opens a proposal there.
SLUG=$(field "$(json "$(req ana POST $A /api/servers '{"name":"Agora"}')")" slug)
{ [ -n "$SLUG" ] && [ "$SLUG" != None ]; } && pass "ana founded server '$SLUG' on node1" || fail "found server failed"
PID=$(field "$(json "$(req ana POST $A /api/servers/$SLUG/proposals '{"kind":"CreateRole","name":"mods"}')")" id)
{ [ -n "$PID" ] && [ "$PID" != None ]; } && pass "ana opened proposal #$PID (CreateRole = RuleChange)" || fail "propose failed"
# The proposal must reach node2 over the Server-scope feed before a vote can route.
pok=""; for i in $(seq 1 60); do
  json "$(req ana GET $B /api/servers/$SLUG/proposals)" | grep -q "\"id\":$PID" && { pok=1; break; }; sleep 0.5
done
[ -n "$pok" ] && pass "proposal replicated node1→node2" || fail "proposal not seen on node2"
# ana casts her ballot via NODE2 — which does NOT own the server, so it must sign and
# FORWARD the CastVote to node1 (the proposal's server owner), which re-checks her
# citizenship and mints the canonical vote. This is the M5 vote-forwarding headline.
is "$(code "$(req ana POST $B /api/proposals/$PID/vote '{"is_aye":true}')")" 200 "ana's vote forwarded node2→node1 (server owner applies)"
# The canonical tally, read back on node1, shows the forwarded aye.
aye=$(json "$(req ana GET $A /api/servers/$SLUG/proposals)" | python3 -c "import sys,json;print(next((p['aye'] for p in json.load(sys.stdin) if p['id']==$PID),'?'))" 2>/dev/null)
is "$aye" 1 "node1 recorded the forwarded vote in the canonical tally (aye)"

echo "== 10. SECURITY: federation node-only endpoints (feed + command) =="
# The signed change-feed and command endpoints are the cluster's trust boundary.
BODY='{"node":1,"body":"{\"CastVote\":{\"proposal\":1,\"voter\":2,\"aye\":true}}","issued_at":0,"nonce":"probe","signature":"'"$(printf '0%.0s' $(seq 1 128))"'"}'
is "$(raw -X GET "$FA/federation/changes?since=0&limit=5")" 401 "feed: no bearer token → 401"
is "$(raw -H 'Authorization: Bearer wrong' -X GET "$FA/federation/changes?since=0&limit=5")" 401 "feed: wrong bearer token → 401"
is "$(raw -H "Authorization: Bearer $TOKEN" -X GET "$FA/federation/changes?since=0&limit=5")" 200 "feed: correct token → 200 (positive control)"
is "$(raw -H 'content-type: application/json' -d "$BODY" -X POST "$FA/federation/command")" 401 "command: no bearer token → 401"
is "$(raw -H 'Authorization: Bearer wrong' -H 'content-type: application/json' -d "$BODY" -X POST "$FA/federation/command")" 401 "command: wrong bearer token → 401"
in4xx "$(raw -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d 'not-json' -X POST "$FA/federation/command")" "command: correct token + malformed body → 4xx (no crash)"
# THE crypto gate: with the right bearer token but a FORGED Ed25519 signature on a
# command claiming to be node 1, the owner still refuses it. Knowing the cluster
# token is not enough to inject a write — you also need node 1's private key.
is "$(raw -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "$BODY" -X POST "$FB/federation/command")" 422 "command: valid token + forged signature → 422 (crypto gate holds)"
# A command signed by a node whose key was never published to the control plane.
GHOST='{"node":99,"body":"{}","issued_at":0,"nonce":"ghost","signature":"'"$(printf '0%.0s' $(seq 1 128))"'"}'
isnt "$(raw -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "$GHOST" -X POST "$FB/federation/command")" 204 "command: unknown/unpublished signer → refused"

echo "== 11. SECURITY: web auth boundaries (across both nodes) =="
# The session identity comes from the signed `sid` cookie, never a path/param.
is "$(raw -X GET "$A/api/social/ana")" 401 "unauthenticated read → 401"
is "$(raw -b "$JAR/ana" -X GET "$A/api/social/ben")" 403 "IDOR read: ana cannot read ben's social → 403"
is "$(code "$(req ana POST $A /api/social/ben/block/cyd)")" 403 "IDOR write: ana cannot act as ben → 403"
is "$(raw -b "$JAR/ana" -X GET "$A/api/social/ben/with/cyd")" 403 "IDOR: ana cannot read ben↔cyd conversation → 403"
# The auth boundary must hold on the OTHER node too (shared secret authenticates ana,
# but she still is not ben) — a federation-specific check.
is "$(code "$(req ana GET $B /api/social/ben)")" 403 "cross-node IDOR: ana@node2 cannot be ben → 403"
is "$(raw -b "$JAR/ana" -X POST "$A/api/social/ana/block/ben")" 403 "CSRF: mutation without token → 403"
is "$(raw -b "$JAR/ana" -b 'dc_csrf=aaa' -H 'x-dc-csrf-token: bbb' -X POST "$A/api/social/ana/block/ben")" 403 "CSRF: cookie/header mismatch → 403"
is "$(raw -b 'sid=forged.session.value' -X GET "$A/api/social/ana")" 401 "forged session cookie → 401"
# Body-size cap: a >64KB body is rejected (413) before any handler runs — a cheap
# memory-exhaustion attempt can't reach the JSON parser. (Body sent from a file so a
# 70KB shell argument can't perturb the probe.)
BIGF="$JAR/bigbody.json"
{ printf '{"handle":"a","password":"'; printf 'x%.0s' $(seq 1 70000); printf '"}'; } >"$BIGF"
is "$(raw -b 'dc_csrf=z' -H 'x-dc-csrf-token: z' -H 'content-type: application/json' --data-binary "@$BIGF" -X POST "$A/api/register")" 413 "oversized body → 413 (DoS cap)"
# Hardening headers on an ordinary response.
HDRS=$(curl -s -D - -o /dev/null "$A/api/config")
echo "$HDRS" | grep -qi 'content-security-policy' && pass "CSP header present" || fail "CSP header missing"
echo "$HDRS" | grep -qi 'x-content-type-options: nosniff' && pass "X-Content-Type-Options: nosniff" || fail "nosniff missing"
echo "$HDRS" | grep -qi 'x-frame-options: DENY' && pass "X-Frame-Options: DENY" || fail "X-Frame-Options missing"

echo "== 12. SECURITY: governance authority (a forwarder cannot fake authority) =="
# ben is not a citizen (not even a member) of agora. His ballot, cast via node2, is
# FORWARDED to node1 (the server owner), which re-checks citizenship and REFUSES it —
# the forwarding node is trusted only to relay, never to decide who may vote.
is "$(code "$(req ben POST $B /api/proposals/$PID/vote '{"is_aye":true}')")" 400 "non-citizen's forwarded vote is refused by the owner"
# A non-citizen likewise cannot open a proposal.
is "$(code "$(req ben POST $B /api/servers/$SLUG/proposals '{"kind":"CreateRole","name":"x"}')")" 400 "non-citizen cannot open a proposal"
# THE hard invariant: citizenship is EARNED, never granted. ben joins agora and tries
# to enfranchise at once — account-age / membership / contribution criteria are unmet,
# so he is not admitted. There is no manual grant path anywhere in the system.
req ben POST $A /api/servers/$SLUG/join >/dev/null 2>&1
adm=$(field "$(json "$(req ben POST $A /api/servers/$SLUG/enfranchise)")" is_admitted)
is "$adm" False "citizenship cannot be self-granted (unmet criteria ⇒ not admitted)"

echo "== 13. SECURITY: account & input hardening =="
# No user enumeration: a wrong password and an unknown handle are indistinguishable.
login(){ raw -b "dc_csrf=$CSRF" -H "x-dc-csrf-token: $CSRF" -H 'content-type: application/json' -d "{\"handle\":\"$2\",\"password\":\"$3\"}" -X POST "$1/api/login"; }
c1=$(login $A ana wrong-password-xxxx)
c2=$(login $A nobody-here-xxx wrong-password-xxxx)
{ [ "$c1" = "$c2" ] && [ "$c1" = 401 ]; } && pass "no user enumeration (wrong-pass == unknown-user == $c1)" || fail "enumeration differs (pass=$c1 user=$c2)"
# A duplicate registration is refused (handle already taken).
in4xx "$(code "$(reg ana $A)")" "duplicate registration refused"
# Self-directed relationships are refused.
is "$(code "$(dm ana $A ana)")" 400 "self-DM refused"
is "$(code "$(req ana POST $A /api/social/ana/block/ana)")" 400 "self-block refused"
# Auth rate limit (AUTH_MAX=10/min per IP): a login burst on node2 trips 429.
got429=""; for i in $(seq 1 15); do
  [ "$(login $B ben nope-nope-nope-xx)" = 429 ] && { got429=1; break; }
done
[ -n "$got429" ] && pass "auth rate limit trips 429 under a burst" || fail "no 429 under auth burst"

echo "== 14. RESILIENCY: federation endpoint robustness =="
# Wrong HTTP verb on the node-only endpoints is a clean 405, not a 500.
is "$(raw -H "Authorization: Bearer $TOKEN" -X POST "$FA/federation/changes?since=0&limit=5")" 405 "wrong method on feed endpoint → 405"
is "$(raw -H "Authorization: Bearer $TOKEN" -X GET "$FA/federation/command")" 405 "wrong method on command endpoint → 405"
# A pathological limit is clamped, never fatal.
is "$(raw -H "Authorization: Bearer $TOKEN" -X GET "$FA/federation/changes?since=0&limit=999999999")" 200 "huge feed limit clamped → 200"
# Malformed query params are a clean 4xx, not a panic/5xx.
in4xx "$(raw -H "Authorization: Bearer $TOKEN" -X GET "$FA/federation/changes?since=abc&limit=xyz")" "malformed feed params → 4xx (no crash)"

echo "== 15. SECURITY: social-graph authority & cross-home DM policy =="
# Uses dot@node1, eli@node2, gus@node2 (registered up front in §4 to dodge §13's
# auth-rate-limit saturation). eli is neither friend nor block of dot.
# --- friend-graph authority (same-home on node2, deterministic) ---
is "$(code "$(fr eli $B eli)")" 400 "self friend-request refused"
is "$(code "$(fa eli $B gus)")" 400 "accept with no pending request refused"
is "$(code "$(fr gus $B eli)")" 200 "gus→eli friend request submitted"
is "$(code "$(fr gus $B eli)")" 200 "duplicate friend request is idempotent"
is "$(code "$(fa gus $B eli)")" 400 "wrong party cannot accept (requester is not the addressee)"
is "$(code "$(fa eli $B gus)")" 200 "addressee accepts the pending request"

# --- the policy endpoint is self-only (IDOR, checked across nodes) ---
pol(){ req "$1" POST "$2" /api/social/"$3"/policy "{\"is_friends_only\":$4}"; }
is "$(code "$(pol dot $B eli true)")" 403 "IDOR: dot cannot set eli's DM policy"

# --- friends-only DM policy is a REPLICATED, cross-home-enforced control ---
# dot (node1) and eli (node2) are neither friends nor blocked. The DM gate runs on the
# SENDER's home (node1 owns UserHome(dot)), so it reads eli's policy from node1's
# REPLICA — the block only takes hold once eli's mutation replicates node2→node1.
is "$(code "$(dm dot $A eli)")" 200 "baseline: dot->eli DM allowed under 'everyone'"
is "$(code "$(pol eli $B eli true)")" 200 "eli sets friends-only (on eli's home node2)"
is "$(field "$(json "$(req eli GET $B /api/social/eli)")" is_friends_only)" True "node2 shows eli friends-only"
den=""; for i in $(seq 1 40); do [ "$(code "$(dm dot $A eli)")" = 400 ] && { den=1; break; }; sleep 0.5; done
[ -n "$den" ] && pass "friends-only replicated node2->node1 and enforced (dot->eli refused)" \
             || fail "cross-home friends-only NOT enforced (dot->eli still allowed)"
is "$(code "$(pol eli $B eli false)")" 200 "eli reverts to 'everyone'"
alw=""; for i in $(seq 1 40); do [ "$(code "$(dm dot $A eli)")" = 200 ] && { alw=1; break; }; sleep 0.5; done
[ -n "$alw" ] && pass "revert replicated node2->node1 — dot->eli allowed again" \
             || fail "revert not enforced (dot->eli still refused)"

echo "== 16. SERVER INVITES: private server + code admits a member =="
# ana founds a PRIVATE server on node1; dot (also node1) joins only via an invite code.
cab=$(field "$(json "$(req ana POST $A /api/servers '{"name":"Cabal","is_private":true}')")" slug)
{ [ -n "$cab" ] && [ "$cab" != None ]; } && pass "ana founded private server '$cab'" || fail "found private server failed"
# Private servers are hidden from the public directory.
curl -s $A/api/servers/public | grep -q "\"slug\":\"$cab\"" && fail "private server leaked into the public directory" || pass "private server hidden from public directory"
# A non-member cannot mint an invite for it.
is "$(code "$(req ben POST $A /api/servers/$cab/invites)")" 400 "non-member cannot mint an invite"
# ana (member) mints a code; dot redeems it and joins.
CODE=$(field "$(json "$(req ana POST $A /api/servers/$cab/invites)")" code)
{ [ -n "$CODE" ] && [ "$CODE" != None ]; } && pass "ana minted an invite code" || fail "mint invite failed"
is "$(code "$(req dot POST $A /api/invites/accept "{\"code\":\"$CODE\"}")")" 200 "dot redeemed the code and joined"
req dot GET $A /api/servers | grep -q "\"slug\":\"$cab\"" && pass "dot's servers now include the private server" || fail "dot did not join"
# A bogus code is refused.
is "$(code "$(req ben POST $A /api/invites/accept '{"code":"not-a-real-code"}')")" 400 "a bogus invite code is refused"

echo "== 17. RESILIENCY: transient etcd partition (lease must survive) =="
# Freeze etcd for a blip shorter than the lease TTL. The nodes must keep serving local
# reads (those never touch etcd) AND retain their leases — a brief control-plane
# hiccup must not silently evict a healthy node and trigger a spurious failover. This
# exercises the reconnecting lease-keepalive (a naive keepalive that gives up on the
# first error would let the lease lapse here).
# grep -c prints the count AND exits 1 when it is zero, so swallow the status with
# `|| true` (never `|| echo 0`, which would append a second line) and default to 0.
promos_before=$(grep -c "rehome: promoted" "$LOG2" 2>/dev/null || true); promos_before=${promos_before:-0}
docker pause "$ETCD_NAME" >/dev/null
is "$(code "$(req ana GET $A /api/social/ana)")" 200 "local reads keep serving during an etcd outage"
sleep 8
docker unpause "$ETCD_NAME" >/dev/null
sleep 20   # past the lease TTL — if renewal had died, node1 would be evicted by now
promos_after=$(grep -c "rehome: promoted" "$LOG2" 2>/dev/null || true); promos_after=${promos_after:-0}
new_promos=$((promos_after - promos_before))
[ "$new_promos" = 0 ] && pass "etcd blip did not evict node1 (no spurious failover)" \
                      || fail "node1 wrongly rehomed after a transient blip ($new_promos new promotions)"
is "$(code "$(dm cyd $A ana)")" 200 "cross-node writes resume after the etcd blip"

echo "== 18. FAILOVER: automatic rehoming when a home node dies =="
# Kill node 1 (it homes ana). node 2 is ana's designated standby, so once node 1's
# etcd lease lapses the rehoming controller on node 2 must PROMOTE itself for ana's
# home — and a write that needs it starts working again with NO human/restart.
kill "$N1" 2>/dev/null; wait "$N1" 2>/dev/null; N1=""
for i in $(seq 1 20); do curl -sf $A/api/config >/dev/null 2>&1 || break; sleep 0.3; done
is "$(code "$(req ben GET $B /api/social/ben)")" 200 "node2 still serves its own users while node1 is down"
# During the lease window ana's home is still owned by the (unreachable) node 1, so a
# 2-home write needing it must FAIL loudly — never a silent half-success.
isnt "$(code "$(fr ben $B ana)")" 200 "before rehoming: write needing the downed home does not report success"
echo "  waiting for node1's etcd lease (15s) to lapse and node2 to auto-promote…"
# Poll until the same write succeeds — proof node2 promoted itself for ana's home.
ok=""; for i in $(seq 1 40); do [ "$(code "$(fr ben $B ana)")" = 200 ] && { ok=1; break; }; sleep 1; done
[ -n "$ok" ] && pass "AUTO-FAILOVER: node2 promoted ana's home; write succeeds with node1 still down" \
             || fail "node2 did not auto-promote ana's home"
grep -q "rehome: promoted" "$LOG2" && pass "node2 logged the rehome promotion" || fail "no rehome promotion logged"
# node 1 returns: the epoch bump must FENCE it — it may not seize ana's home back, and
# the cluster stays consistent (node 1 now forwards ana's writes to node 2).
echo "  restarting node1 — it must be fenced, not reclaim ana…"
launch1
for i in $(seq 1 40); do curl -sf $A/api/config >/dev/null 2>&1 && break; sleep 0.3; done
sleep 6   # let node1 re-federate and its reconciler try (and fail) to reclaim
# Durability: node1's own founded server survived the crash (loaded back from disk).
# (agora is public, so it shows in the unauthenticated public directory.)
curl -s $A/api/servers/public | grep -q '"slug":"agora"' && pass "durability: node1's data (agora) survived the restart" || fail "node1 lost data across restart"
is "$(code "$(fr ben $B ana)")" 200 "after node1 returns: ana's home still served (node1 fenced)"
is "$(code "$(fr ana $A ben)")" 200 "returned node1 forwards ana's writes to the new owner (node2)"

echo
echo "==================  $PASS passed, $FAIL failed  =================="
[ "$FAIL" -eq 0 ]
