# democrachat — Governance Model

> A chat platform where each **server** is a self-governing
> polity. The powers a Discord owner holds (bans, custom emojis, channels, the
> rulebook) are instead **ballots** decided by that server's **citizens**. This
> document defines the governance engine, re-implemented from the democratos spec
> and adapted for chat.

## 1. Core concept

- The platform hosts many **servers**.
- Within a server, a user holds a **tier**: `Guest → Member → Citizen`.
- **Citizens** are the electorate. They govern the server.
- The **franchise** (becoming a Citizen) is *earned* by meeting **criteria**.
- The criteria — and the rules, thresholds, and governance surface — are **set by
  the citizens themselves**.

### Two hard invariants

1. **Citizenship is criteria-only, and automatic.** The *only* way to reach
   `Tier::Citizen` is to satisfy the server's [`FranchiseCriteria`], as judged by
   `evaluate_eligibility` — and once a member does, citizenship is **conferred
   automatically** (`Services::auto_enfranchise`, swept on reads), never requested.
   There is **no** proposal, role, admin action, or invite anywhere in the domain
   that grants the franchise. The sole structural exception is the **founder**, who
   is Citizen #1 so a server of one can exist at all — an influence that *dilutes*
   automatically as the server grows and which never extends to enfranchising anyone
   *else*. Weighting a vote (`GrantVoteWeight`) only ever adjusts an *already*-
   enfranchised citizen's ballot; it is not a path into the franchise.
2. **Different servers vote on different things.** Each server enables a subset of the
   platform's ballot kinds — its **governance surface** (`Server::enabled_ballots`).
   One server puts only bans to a vote; another votes on which custom emojis exist;
   another governs its whole rulebook and jury policy. Two kinds are always on so a
   server can never wall itself off from self-rule: amending the franchise criteria,
   and changing the surface itself.

## 2. The governing principle

> **Make takeover slow, not impossible.**

Every defense is a **time tax**. None denies anyone the franchise — they only slow
the *rate* at which the electorate can flip. A brigade loses interest; sybils age
poorly; a genuine change of heart persists. We target the *speed* of change, never
the outcome.

## 3. The four defensive layers

A flood must beat **all four**, and each costs weeks.

1. **Earned franchise** (`evaluate_eligibility`) — by default, server dwell ≥ 28d and
   no active sanction. A server's electorate can vote to add stricter gates (a minimum
   account age, a minimum of endorsement-weighted contribution) on top. The one
   exception is a server's **founding cohort**: while it is still in Seed, whoever
   joins is enfranchised on arrival, because a brand-new community that cannot vote
   for a month is not a community. That waiver covers the *wait* only — never a
   sanction, a franchise bar, or a contribution bar — and shuts by itself at five
   citizens, so it can seat the founder plus four.
2. **Enfranchisement rate cap** (`enfranchisement_slots`) — the citizen roll grows
   by at most +10% / 30 days (floor +5). Qualified newcomers beyond the cap queue by
   qualification date; nobody is denied, only delayed.
3. **Tiered thresholds** (`threshold_for`) — routine moderation is a simple
   majority; bans/timeouts/recall need 60% + quorum; constitutional changes need
   ~2/3 + 50% quorum, and are disabled entirely in the Seed phase.
4. **Timelock + recall** (`Proposal::close`) — a passed constitutional change waits
   out a 7-day recall window before it takes effect.

## 4. Bootstrap phases (training wheels)

Small servers are where capture is easiest and percentage-math weakest, so a new
server runs on training wheels, derived purely from its citizen count:

| Phase | Citizens | Governance |
|---|---|---|
| **Seed** | 1–4 | Founder is Citizen #1 and may **provisionally** set the server up (rules, emojis, channels). No constitutional amendments. Whoever joins is enfranchised at once — the founding cohort. |

The counts are the platform defaults. An operator sets their own with
`DEMOCRACHAT_CHARTERING_AT` / `DEMOCRACHAT_SOVEREIGN_AT`; the chartering threshold
is doubly load-bearing, since it bounds the founding cohort *and* decides when
amendments unlock. Under soft email verification, `DEMOCRACHAT_FRANCHISE_GRACE_DAYS`
sets how long a founding member's unconfirmed vote stands (`0` = confirm first).
| **Chartering** | 5–24 | Amendments may be proposed under **stricter** thresholds; provisioning becomes ballots. |
| **Sovereign** | 25+ | Full self-governance; percentage math works naturally. |

## 5. The governance surface (`BallotKind`)

`ProposalKind` is the set of votable actions; each maps to a `BallotKind`
discriminant and a `DecisionClass`. A server enables the subset it governs. Kinds
today include: `RemoveContent`, `Ban`, `Timeout`, `Recall`, `AddEmoji`,
`RemoveEmoji`, `AddRule`, `RemoveRule`, `AmendCriteria`, `SetJurySizing`,
`SetVoteWeighting`, `SetWeightingScope`, `GrantVoteWeight`, `SetGovernanceSurface`.
Chat-native kinds (channel create/delete, slow-mode, roles, pins) are added as the
chat model lands (M3).

### Roles are earned, not assigned

A ballot decides which custom roles *exist* (`CreateRole` sets a role's name and its
[`RoleCriteria`]; `DeleteRole` removes it) — but **no ballot, request, or admin
action assigns a member to one**. Like the franchise, role membership is *derived*:
a member holds a role automatically the moment they meet its criteria (server dwell,
endorsed contribution, and/or citizenship), and loses it automatically if they fall
below. There are no assignment records; `RoleService` computes a role's holders on
read. This keeps roles pure addressing groups — they carry no vote or permission —
while making "all roles apply automatically when conditions are met" structural
rather than a matter of remembering to click.

The **one** exception to *automatic* is opting **out**: the `@moderator` role is a
duty, not just a label, so a member may set `has_declined_moderator` and never hold
it even while qualified (the sole self-serve role control). Every other role is
non-refusable — if you meet its conditions, you hold it.

## 6. Trial by jury (fast-path moderation)

Spam/abuse can't wait days for a server ballot. A report may go to a **jury**:
`select_jury` draws a deterministic, seeded, auditable panel of members (excluding
the accused); conviction needs a **2/3 supermajority of the whole jury**. A guilty
verdict sanctions the accused (which disqualifies them from the franchise) and
removes the content. The panel is always a strict **minority** of the electorate
(`JurySizing`), so big servers aren't dragged into every report.

## 7. Contribution comes from endorsement (M1)

The `contribution` score that Layer 1 consumes is not a raw like count. It is
**endorsement-weighted**: only a **franchised citizen's** reaction to *another*
member's message raises that author's contribution, and only once per
(message, citizen) no matter how many emojis they add. Withdrawing the reaction
withdraws the endorsement. A mere Member reacting, or an author reacting to
themselves, does nothing. This is the concrete, gameable-resistant answer to the
open question "how is 'positively received by citizens' measured" — earning the
franchise means citizens, not the crowd, valued your contributions.

## 8. Roadmap

- **M0 (done):** the governance engine, provable over the CLI.
- **M1 (done):** the chat model — servers, channels, messages, threaded replies,
  reactions — with citizen reactions driving the franchise contribution score.
- **M2 (done):** a JS-first realtime web client + WebSocket gateway over the same
  use-cases (`adapter-web`), served by `democrachat serve`. Live messages,
  reactions, presence; a governance panel showing your standing and the server's
  ballot surface. `--dev` adds clock fast-forward + simulated endorsements so the
  earn-the-vote flow is demoable without waiting 30 days.
- **M3:** chat-native ballot kinds (channel create/delete, slow-mode, pins)
  wired into the per-server governance surface.
- **M4:** franchise-decoupled roles + voice presence. (Voice media deferred.)

## 8. Open questions (starting defaults, not findings)

- How exactly "positively received by citizens" (contribution) is measured and
  resists gaming.
- Identity friction that raises sybil cost without real-world ID.
- Inter-server brigading defenses.
- Every number here (30d, 14d, 10%, 2/3, 7d, phase sizes) is a default to validate.
