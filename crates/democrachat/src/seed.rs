//! The `--demo` testbed seed: one server that exercises every kind of channel and
//! content, in a deterministic known-good state. Wiped and rebuilt on demand by
//! `scripts/reset-testbed.sh`, it backs both manual click-through testing and
//! automated runs (log in as any seeded account with [`DEMO_PASSWORD`]).
//!
//! Everything is best-effort (`let _ = …`): a seed is dev scaffolding, so a single
//! failed step degrades the world rather than aborting the boot. The one hard
//! requirement is that the server gets founded; the rest layers on top.

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Clock, MembershipStore, ServerStore, Services, UserStore};
use domain::{Attachment, ProposalKind, Tier};

/// The password set on every seeded account so a tester can sign in as anyone.
pub const DEMO_PASSWORD: &str = "democrachat-demo!";

/// The testbed server's slug (derived from its name by the founder flow).
const SLUG: &str = "testbed";

/// Build the demo world if the store is empty. Idempotent: a populated store is
/// left untouched (so a `--demo` restart on an unwiped snapshot is a no-op).
pub async fn seed_demo(services: &Services, store: &MemoryStore, clock: &FixedClock) {
    if !services.list_servers().await.is_empty() {
        return;
    }
    let now = clock.now();

    // ── Accounts ──────────────────────────────────────────────────────────
    // ada founds and is citizen #1; grace/hiro/mimi/nova become citizens (5 =>
    // Chartering, where governance is live); otto/pax stay members; troll is the
    // ban target.
    let everyone = ["ada", "grace", "hiro", "mimi", "nova", "otto", "pax", "troll"];
    for h in everyone {
        let _ = services.register_account(h).await;
    }
    let _ = services.found_server("ada", "Testbed").await;

    // ── Channels: every kind ─────────────────────────────────────────────
    // Provision while the server is still Seed (founder-only power, lost once the
    // citizen roll reaches Chartering). #general + #appeals come with the founding;
    // add a second text channel, a voice channel, a governance channel, and an
    // end-to-end-encrypted one.
    let _ = services.chat().create_channel("ada", SLUG, "ideas", "brainstorms & planning").await;
    let _ = services.chat().create_channel("ada", SLUG, "governance", "how we run this place").await;
    let _ = services.chat().create_voice_channel("ada", SLUG, "voice-lounge", "hop in and talk").await;
    // An encryption-ready channel left as plaintext: end-to-end encryption is
    // established *client-side* (the browser mints the channel key and seals it to
    // each member's device), so a server-side seed can't hand anyone a key. Turn it
    // on from the UI's "Encrypt" control to distribute keys to the current members.
    let _ = services.chat().create_channel("ada", SLUG, "secret", "click Encrypt to make this end-to-end encrypted").await;

    // Now bring in the rest and seat four as citizens (5 total => Chartering, where
    // governance is live). otto/pax stay members; troll is the ban target.
    for h in &everyone[1..] {
        let _ = services.join_server(h, SLUG).await;
    }
    // Backdate the founder too, so the whole seeded electorate sits outside the
    // 30-day enfranchisement window and testers can enfranchise freely (see
    // `seat_citizen`).
    for h in ["ada", "grace", "hiro", "mimi", "nova"] {
        seat_citizen(store, clock, h).await;
    }
    // otto and pax stay recent members (join date = now): they demonstrate the
    // member tier and the "not eligible yet" state, and can't vote. (ratbum, added
    // last, is backdated so it auto-enfranchises on first load — see below.)

    // ── Discovery tags (server, channel, user) ───────────────────────────
    let _ = services.tags().set_server_tags("ada", SLUG, "demo, testing, showcase").await;
    let _ = services.tags().set_channel_tags("ada", SLUG, "ideas", "planning, roadmap").await;
    let _ = services.tags().set_user_tags("grace", "designer, rustacean").await;

    // ── Custom emoji (curated by continuous vote, not ballots) ───────────
    let glyph = |g: &str| {
        format!(
            "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' \
             width='64' height='64'><text x='6' y='52' font-size='52'>{g}</text></svg>"
        )
    };
    let _ = services.emoji().add_emoji("ada", SLUG, "party", &glyph("🎉")).await;
    let _ = services.emoji().add_emoji("grace", SLUG, "ship", &glyph("🚀")).await;
    let _ = services.emoji().add_emoji("hiro", SLUG, "eyes", &glyph("👀")).await;
    // A couple of endorsing votes so the ranking has movement.
    for id in [1u64, 2] {
        let _ = services.emoji().vote_emoji("mimi", SLUG, id, true).await;
        let _ = services.emoji().vote_emoji("nova", SLUG, id, true).await;
    }

    // ── Messages: plain, markdown, spoiler, mention, emoji, replies ──────
    let _ = post(services, "ada", "general", "Welcome to the **Testbed** — every feature lives in this server.").await;
    let fmt = post(
        services,
        "grace",
        "general",
        "Markdown works: **bold**, _italic_, `inline code`, and lists:\n- one\n- two\n- three",
    )
    .await;
    let _ = post(services, "hiro", "general", "Careful, spoilers: ||the butler did it||").await;
    let _ = post(services, "mimi", "general", "Hey @ada — love the :party: plan 🎉 ping me @grace").await;
    let _ = post(services, "nova", "ideas", "We should add per-channel dark mode.").await;
    let _ = post(services, "otto", "ideas", "+1, and a saved-search feature.").await;
    // A threaded reply + endorsing reactions on the formatted message.
    if let Ok(parent) = &fmt {
        let _ = services.chat().reply_message("nova", parent.id.0, "Clean formatting, nice.").await;
        let _ = services.chat().react("hiro", parent.id.0, "🚀").await;
        let _ = services.chat().react("mimi", parent.id.0, "👍").await;
        let _ = services.chat().react("ada", parent.id.0, "🎉").await;
    }

    // ── Media attachments: an image (re-encoded) and a spoilered image ───
    if let Some(att) = image_attachment(services, "the shiny new logo", false).await {
        let _ = services
            .chat()
            .post_message_with_attachments("ada", SLUG, "general", "Shipping the logo:", vec![att])
            .await;
    }
    if let Some(att) = image_attachment(services, "spoiler pic", true).await {
        let _ = services
            .chat()
            .post_message_with_attachments("grace", SLUG, "general", "Behind the spoiler:", vec![att])
            .await;
    }

    // ── Governance history: pass/fail/open, via clock time-travel ────────
    // Open a batch of ballots now, decide them by advancing past the voting
    // window, then restore the clock so live views read the real time.
    let citizens = ["ada", "grace", "hiro", "mimi", "nova"];
    if let Some(uid) = user_id(services, "troll").await {
        pass(services, &citizens, ProposalKind::Ban { user: uid }).await; // troll -> banned
    }
    if let Some(uid) = user_id(services, "pax").await {
        pass(services, &citizens, ProposalKind::Mute { user: uid }).await; // pax -> muted
    }
    if let Some(uid) = user_id(services, "hiro").await {
        pass(services, &citizens, ProposalKind::AppointPolice { user: uid }).await; // hiro -> police
    }
    pass(services, &citizens, ProposalKind::AddRule { text: "Be excellent to each other.".into() }).await;
    pass(services, &citizens, ProposalKind::AddRule { text: "Argue the point, not the person.".into() }).await;
    // The moderator role: earned automatically by any citizen who has been a member
    // 60+ days — no assignment step — and declinable by the member (the sole opt-out).
    pass(
        services,
        &citizens,
        ProposalKind::CreateRole {
            name: "moderator".into(),
            criteria: domain::RoleCriteria {
                min_account_age_days: 0,
                min_membership_days: 60,
                min_contribution: 0,
                requires_citizen: true,
            },
        },
    )
    .await;
    // A ballot the electorate rejects (proposer alone in favour) — an archived fail.
    if let Some(uid) = user_id(services, "otto").await {
        fail(services, &citizens, ProposalKind::Ban { user: uid }).await;
    }

    // Advance twice: past the voting window (standard effects apply) and past the
    // recall timelock (constitutional effects apply), then rewind to real time.
    clock.set(now.plus_days(5));
    services.governance().resolve_due(SLUG).await;
    clock.set(now.plus_days(14));
    services.governance().resolve_due(SLUG).await;
    clock.set(now);

    // ── Live ballots (opened at the restored clock, so they stay open) ────
    // A plain open ballot with a couple of votes already cast, so there is
    // something to vote on and to watch the tally on.
    if let Ok(p) = services
        .governance()
        .open_proposal("grace", SLUG, ProposalKind::CreateChannel {
            name: "lounge".into(),
            topic: "somewhere casual".into(),
            is_voice: false,
        })
        .await
    {
        let _ = services.governance().cast_vote("grace", p.id.0, true).await;
        let _ = services.governance().cast_vote("hiro", p.id.0, true).await;
        let _ = services.governance().cast_vote("mimi", p.id.0, false).await;
    }

    // An open ballot that was *amended* after votes were cast — the amendment
    // reset the deadline to 24h and nullified those votes (so this one shows a
    // two-change bundle at zero-zero, ready to re-vote).
    if let Ok(p) = services
        .governance()
        .open_proposal("ada", SLUG, ProposalKind::AddRule { text: "Keep #general on-topic.".into() })
        .await
    {
        let _ = services.governance().cast_vote("ada", p.id.0, true).await;
        let _ = services.governance().cast_vote("grace", p.id.0, true).await;
        let _ = services
            .governance()
            .amend_proposal("ada", p.id.0, ProposalKind::AddRule { text: "No unlabelled spoilers.".into() })
            .await;
    }

    // ── Social graph: a friendship and a block ───────────────────────────
    let _ = services.social().request_friend("ada", "grace").await;
    let _ = services.social().accept_friend("grace", "ada").await;
    let _ = services.social().block_user("ada", "troll").await;

    // ── Sign-in for every account ────────────────────────────────────────
    for h in everyone {
        let _ = services.set_password(h, DEMO_PASSWORD).await;
    }

    // A named account with its own password — a known handle to sign in as. Its
    // join date is backdated past the 28-day gate, so on first load the automatic
    // enfranchisement sweep makes it a citizen and it can vote on the open ballots
    // straight away (no clock fast-forward, which would close those ballots first).
    let _ = services.register_account("ratbum").await;
    let _ = services.join_server("ratbum", SLUG).await;
    predate_membership(store, clock, "ratbum").await;
    let _ = services.set_password("ratbum", "p4sswordp4ssword!").await;
}

/// Backdate a member's join date past the default 28-day franchise gate (to 30 days
/// ago), leaving their tier untouched. They become immediately eligible to
/// enfranchise, so a tester can exercise the guest→member→citizen→vote path without
/// fast-forwarding the clock (which would close the open ballots first). Dev only.
async fn predate_membership(store: &MemoryStore, clock: &FixedClock, handle: &str) {
    let (Some(user), Some(server)) = (
        store.find_by_handle(handle).await.ok().flatten(),
        store.find_by_slug(SLUG).await.ok().flatten(),
    ) else {
        return;
    };
    if let Some(mut m) = store.get(user.id, server.id).await.ok().flatten() {
        m.joined_at = clock.now().plus_days(-30);
        let _ = store.upsert(m).await;
    }
}

/// Promote a member to citizen directly in the store (bypassing the earned-franchise
/// flow) so the demo reaches Chartering deterministically, without fighting the
/// rate cap or account-age gates. Dev seed only.
///
/// The seeded citizens are backdated well outside the 30-day rate-cap window, so
/// they don't count as recent admissions — otherwise five same-day citizens would
/// consume the whole admission cap and any tester trying to enfranchise would hit
/// "the rate cap is full this window."
async fn seat_citizen(store: &MemoryStore, clock: &FixedClock, handle: &str) {
    let (Some(user), Some(server)) = (
        store.find_by_handle(handle).await.ok().flatten(),
        store.find_by_slug(SLUG).await.ok().flatten(),
    ) else {
        return;
    };
    if let Some(mut m) = store.get(user.id, server.id).await.ok().flatten() {
        m.tier = Tier::Citizen;
        m.contribution = 8;
        m.enfranchised_at = Some(clock.now().plus_days(-60));
        // Long-standing members, so they clear the @moderator role's 60-day dwell
        // criterion — the seeded moderator roster is populated (and declinable) out
        // of the box rather than empty until someone ages in.
        m.joined_at = clock.now().plus_days(-90);
        let _ = store.upsert(m).await;
    }
}

/// Post a plaintext message, returning it so callers can thread replies/reactions.
async fn post(
    services: &Services,
    handle: &str,
    channel: &str,
    body: &str,
) -> Result<domain::Message, app::MessageError> {
    services.chat().post_message(handle, SLUG, channel, body).await
}

/// Resolve a handle to its user id (for ballots addressed by id).
async fn user_id(services: &Services, handle: &str) -> Option<domain::UserId> {
    services.find_user(handle).await.map(|u| u.id)
}

/// Open a ballot and carry it with a unanimous aye from the citizen roll.
async fn pass(services: &Services, citizens: &[&str], kind: ProposalKind) {
    let Ok(p) = services.governance().open_proposal("ada", SLUG, kind).await else {
        return;
    };
    for c in citizens {
        let _ = services.governance().cast_vote(c, p.id.0, true).await;
    }
}

/// Open a ballot the electorate rejects: the proposer votes aye, everyone else nay.
async fn fail(services: &Services, citizens: &[&str], kind: ProposalKind) {
    let Ok(p) = services.governance().open_proposal("ada", SLUG, kind).await else {
        return;
    };
    for c in citizens {
        let _ = services.governance().cast_vote(c, p.id.0, c == &"ada").await;
    }
}

/// Encode a tiny PNG through the real media pipeline (so it is re-encoded exactly
/// as an upload would be) and wrap it as an attachment, or `None` if storage fails.
async fn image_attachment(services: &Services, caption: &str, is_spoiler: bool) -> Option<Attachment> {
    let img = image::RgbImage::from_fn(16, 16, |x, y| {
        image::Rgb([(x * 16) as u8, (y * 16) as u8, 160])
    });
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .ok()?;
    let (key, content_type, kind) = services.chat().store_media("image/png", buf.get_ref()).ok()?;
    Some(Attachment::new(key, content_type, kind, caption, is_spoiler))
}
