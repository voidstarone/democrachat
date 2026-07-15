# Testbed — a resettable demo environment

A deterministic, throwaway environment for manual and automated testing. One
server ("Testbed") is seeded with every kind of channel and content, in a known
state that is identical after every reset.

## Reset & run

```sh
scripts/reset-testbed.sh            # wipe, reseed, serve in the foreground
PORT=4000 scripts/reset-testbed.sh  # choose the port (default 3939)
scripts/reset-testbed.sh --detach   # run in the background, print the URL
```

The script wipes `./.testbed/` (its own data snapshot + media tier — never your
real `democrachat.json`/`media`), rebuilds, and boots `democrachat serve --demo`.
`--demo` implies `--dev`, so the dev clock control is available.

State lives entirely under `.testbed/` (git-ignored); a reset is a clean `rm`.

## Sign in

Every seeded account shares one password: **`democrachat-demo!`**

| Handle | Standing |
|---|---|
| `ada`   | founder, citizen |
| `grace` | citizen (friend of ada) |
| `hiro`  | citizen, **police** |
| `mimi`  | citizen |
| `nova`  | citizen |
| `otto`  | member |
| `pax`   | member, **muted** |
| `troll` | member, **banned** (blocked by ada) |
| `ratbum` | member with a backdated join date — **auto-enfranchises to citizen on first load** (password `p4sswordp4ssword!`) |

Five citizens puts the server in the **Chartering** phase, so governance is live.

Enfranchisement is automatic: `otto`/`pax` are recent members who can't vote yet
(0 of 28 days' membership), while `ratbum` already clears the 28-day gate, so the
automatic sweep makes it a citizen the moment it loads the server — no "become
citizen" click, and the open ballots are still open to vote on. Use the dev clock
to age `otto`/`pax` and watch them enfranchise on their own.

## What's seeded

**Channels (every kind)** — `#general`, `#ideas`, `#governance` (text),
`voice-lounge` (voice), `#secret` (end-to-end encrypted), `#appeals` (restricted).

**Messages** — plaintext, markdown (bold/italic/code/lists), `||spoilers||`,
`@mentions`, `:emoji:` shortcodes, a threaded reply, emoji reactions, and image
attachments (one normal, one spoilered — both run through the real re-encode
pipeline).

**Governance** — proposals in every state: six passed & applied (a ban, a mute, a
police appointment, two rules, a role), one failed, one still open. Custom emoji
with votes. Discovery tags on the server, a channel, and a user.

The passed **role** is `@moderator`, created with criteria "citizen + 60 days'
membership". Nobody is *assigned* it — holders are derived, so all five citizens
(long-standing members) auto-hold it, while the recent members `otto`/`pax` don't.
Open **Settings → Personal → Roles** and toggle "Never make me a moderator" to opt
out (the one role a member can refuse); it drops you from `@moderator` immediately
even though you still qualify.

**Social** — a friendship (ada↔grace) and a block (ada→troll).

Sealed direct-message *content* is not seeded — DMs are end-to-end encrypted and
their ciphertext is minted client-side (in the browser's WASM crypto), which a
server-side seed can't produce. The DM policy, friendship, and block are seeded;
compose a DM in the browser to exercise the sealed path.

## Automated use

`--detach` blocks until the server answers, then exits 0, so a test harness can:

```sh
PORT=3939 scripts/reset-testbed.sh --detach
# ... drive http://127.0.0.1:3939 ...
pkill -f 'democrachat serve.*--addr 127.0.0.1:3939'
```

Each run reseeds from scratch, so tests start from the same fixture every time.
