# democrachat

A self-governing, Discord-style chat platform where each **server** governs
itself. The powers a Discord owner would hold (bans, custom emojis, channels, the
rulebook) are instead **ballots** decided by that server's **citizens**, a tier
that is *earned by meeting criteria* and can **never** be handed to anyone. It is
the chat-shaped sibling of [democratos](../democratos) (a self-governing Reddit),
re-implemented from the governance spec rather than copied.

Two rules are load-bearing and enforced structurally:

1. **Citizenship is criteria-only.** There is no grant, role, admin action, or
   invite that makes someone a voter — only `evaluate_eligibility`. (The founder is
   Citizen #1 as a bootstrap, and that influence dilutes as the server grows.)
2. **Different servers vote on different things.** Each server enables its own
   **governance surface** — which kinds of decision it puts to a vote.

See [`docs/governance.md`](docs/governance.md) for the full model.

## Architecture — ports & adapters (hexagonal)

```
domain   — pure governance logic (the four defensive layers). No I/O.
app      — use-cases (Services) + the port traits adapters implement.
adapter-store-memory — in-memory store (JSON-persisted) + a controllable clock.
adapter-cli          — a CLI over the same use-cases.
democrachat          — composition root: the only crate that names adapters.
```

`domain` and `app` cannot name a database or a web framework, so storage and
delivery are chosen only in the `democrachat` composition root.

## Status: milestones M0–M2 done

- **M0** — the governance engine (four defensive layers, phases, jury), unit-tested.
- **M1** — the chat model (servers, channels, threaded messages, reactions); a
  **citizen's** reaction endorses a message's author, driving the franchise
  contribution score.
- **M2** — a **runnable realtime web demo**: a JS-first client + a WebSocket
  gateway over the same use-cases.

`cargo test` → 44 domain tests + 6 chat integration tests.

## Run the web demo

```sh
cargo run -p democrachat -- serve --dev
# → http://127.0.0.1:3000   (open two browser tabs to see realtime chat)
```

It auto-seeds a sample server on first run. Pick a handle, join, chat (replies +
reactions), and watch the **Your standing** panel: you start a *guest*, join to
become a *member*, and earn the *citizen* vote only by meeting the criteria. In
`--dev` mode two clearly-labeled shortcuts let you reach that moment without
waiting 30 real days: **fast-forward 15 days** and **simulate citizen
endorsements**. Data persists to `democrachat.json` (override `DEMOCRACHAT_DATA`).

Flags: `--addr <host:port>` (default `127.0.0.1:3000`), `--dev` (enables the clock
and endorsement shortcuts — never use in a real deployment).

> Per the design, there is **no owner and no mods**, and **no way to grant the
> vote** — citizenship is only ever earned by criteria.

## Same use-cases over the CLI

```sh
democrachat register alice
democrachat found alice "Gamers"          # alice becomes Citizen #1
democrachat register bob
democrachat join bob gamers               # bob is a Member — no vote
democrachat enfranchise bob gamers        # ✗ rejected: criteria unmet
democrachat create-channel alice gamers general "welcome"
democrachat post bob gamers general "hello"
democrachat react alice 1 👍              # a citizen endorsement
democrachat thread gamers general
```

`--now <unix>` moves the clock to demonstrate the time-based rules. There is
deliberately no `grant-citizen` command.
