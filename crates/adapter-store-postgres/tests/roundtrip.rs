//! Live end-to-end contract test for the Postgres store adapter.
//!
//! Gated on `DATABASE_URL` — it drops and rebuilds the `public` schema, so it only
//! runs when pointed at a throwaway database, and is a silent no-op otherwise (so
//! `cargo test --workspace` stays green with no database present). Bring one up with:
//!
//! ```text
//! docker run -d --rm --name dcpg -e POSTGRES_PASSWORD=pw -e POSTGRES_DB=democrachat \
//!   -p 55433:5432 postgres:16-alpine
//! DATABASE_URL=postgres://postgres:pw@127.0.0.1:55433/democrachat \
//!   cargo test -p adapter-store-postgres --test roundtrip
//! ```
//!
//! It exercises every port at least once, focusing on the SQL the memory store
//! can't validate: sequence id minting, `ON CONFLICT` upserts and dedup, the
//! `citizen_count`/`admitted_since` column filters, both-direction lookups
//! (conversation, is_blocked_between, friendship between), and the `jsonb_set`
//! revoke.

use adapter_store_postgres::PgStore;
use app::{
    BlockStore, ChannelKeyStore, ChannelStore, DmStore, EmojiStore, EmojiVoteStore, FriendStore,
    InviteStore, KeyDirectoryStore, MembershipStore, MessageStore, ProposalStore, ReactionStore,
    RoleColorVoteStore, RoleStore, RuleStore, ServerStore, UserStore, VoteStore,
};
use domain::{
    enfranchisement_slots, Block, Channel, ChannelKeyGrant, DmMessage, Emoji, EmojiVote, Friendship,
    Invite, Membership, Message, Proposal, ProposalKind, Reaction, Role, RoleAssignment, RoleColor,
    RoleColorVote, Rule, Server, Tier, Timestamp, User, UserKeys, Vote, WrappedKey,
};

const T: Timestamp = Timestamp(1_000);

/// Tests in a binary run on parallel threads, but both reset the one shared schema.
/// This lock serializes them so a reset never lands mid-run of the other test. Held
/// for the whole test body (returned to the caller), released on drop.
fn db_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Wipe and rebuild the schema so each run starts clean, then connect (which runs
/// the DDL). Returns `None` when `DATABASE_URL` is unset — the caller then skips.
async fn fresh_store(max_connections: u32) -> Option<std::sync::Arc<PgStore>> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let reset = sqlx::postgres::PgPool::connect(&url).await.expect("connect for reset");
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(&reset)
        .await
        .expect("reset schema");
    reset.close().await;
    Some(PgStore::connect(&url, max_connections).await.expect("connect PgStore"))
}

#[tokio::test]
async fn every_port_round_trips_against_a_live_postgres() {
    let _guard = db_lock().lock().await;
    let Some(store) = fresh_store(5).await else {
        eprintln!("DATABASE_URL unset — skipping the live Postgres round-trip test");
        return;
    };
    let s = &*store;

    // ── users ────────────────────────────────────────────────────────────────
    let uid_a = UserStore::next_user_id(s).await.unwrap();
    let uid_b = UserStore::next_user_id(s).await.unwrap();
    assert_ne!(uid_a.0, uid_b.0, "the sequence mints distinct ids");
    UserStore::insert_user(s, User::new(uid_a, "ada", T)).await.unwrap();
    UserStore::insert_user(s, User::new(uid_b, "grace", T)).await.unwrap();
    assert_eq!(UserStore::get_user(s, uid_a).await.unwrap().unwrap().handle, "ada");
    assert_eq!(UserStore::find_by_handle(s, "grace").await.unwrap().unwrap().id, uid_b);
    assert!(UserStore::find_by_handle(s, "nobody").await.unwrap().is_none());
    let mut ada = UserStore::get_user(s, uid_a).await.unwrap().unwrap();
    ada.password_hash = "phc".into();
    UserStore::update_user(s, ada).await.unwrap();
    assert_eq!(UserStore::get_user(s, uid_a).await.unwrap().unwrap().password_hash, "phc");
    assert_eq!(UserStore::list_all(s).await.unwrap().len(), 2);

    // duplicate handle is a Conflict, not a panic
    let dup_id = UserStore::next_user_id(s).await.unwrap();
    assert!(matches!(
        UserStore::insert_user(s, User::new(dup_id, "ada", T)).await,
        Err(app::StoreError::Conflict)
    ));

    // ── servers ──────────────────────────────────────────────────────────────
    let sid = ServerStore::next_server_id(s).await.unwrap();
    ServerStore::insert_server(s, Server::new(sid, "town", "Town", uid_a, T)).await.unwrap();
    assert_eq!(ServerStore::find_by_slug(s, "town").await.unwrap().unwrap().id, sid);
    let mut town = ServerStore::get_server(s, sid).await.unwrap().unwrap();
    town.is_private = true;
    ServerStore::update_server(s, town).await.unwrap();
    assert!(ServerStore::get_server(s, sid).await.unwrap().unwrap().is_private);
    assert_eq!(ServerStore::list_all(s).await.unwrap().len(), 1);

    // ── memberships (tier + enfranchised_at column filters) ──────────────────
    let mut m_a = Membership::joined(uid_a, sid, T);
    m_a.tier = Tier::Citizen;
    m_a.enfranchised_at = Some(Timestamp(1_500));
    MembershipStore::upsert(s, m_a).await.unwrap();
    let mut m_b = Membership::joined(uid_b, sid, T);
    m_b.tier = Tier::Member;
    MembershipStore::upsert(s, m_b).await.unwrap();
    assert_eq!(MembershipStore::get(s, uid_a, sid).await.unwrap().unwrap().tier, Tier::Citizen);
    assert_eq!(MembershipStore::list_for_server(s, sid).await.unwrap().len(), 2);
    assert_eq!(MembershipStore::citizen_count(s, sid).await.unwrap(), 1, "only ada is a citizen");
    assert_eq!(MembershipStore::admitted_since(s, sid, Timestamp(1_400)).await.unwrap(), 1);
    assert_eq!(MembershipStore::admitted_since(s, sid, Timestamp(1_600)).await.unwrap(), 0);
    // upsert overwrites (promote grace)
    let mut m_b2 = Membership::joined(uid_b, sid, T);
    m_b2.tier = Tier::Citizen;
    m_b2.enfranchised_at = Some(Timestamp(1_500));
    MembershipStore::upsert(s, m_b2).await.unwrap();
    assert_eq!(MembershipStore::citizen_count(s, sid).await.unwrap(), 2, "upsert promoted grace");

    // ── channels ─────────────────────────────────────────────────────────────
    let cid = ChannelStore::next_channel_id(s).await.unwrap();
    ChannelStore::insert_channel(s, Channel::new(cid, sid, "general", "topic", T)).await.unwrap();
    assert_eq!(ChannelStore::find_by_name(s, sid, "general").await.unwrap().unwrap().id, cid);
    assert_eq!(ChannelStore::list_for_server(s, sid).await.unwrap().len(), 1);
    assert_eq!(ChannelStore::get_channel(s, cid).await.unwrap().unwrap().topic, "topic");

    // ── messages ─────────────────────────────────────────────────────────────
    let mid = MessageStore::next_message_id(s).await.unwrap();
    MessageStore::insert_message(s, Message::new(mid, cid, sid, uid_a, "hi", None, T)).await.unwrap();
    let mut msg = MessageStore::get_message(s, mid).await.unwrap().unwrap();
    msg.body = "edited".into();
    MessageStore::update_message(s, msg).await.unwrap();
    assert_eq!(MessageStore::get_message(s, mid).await.unwrap().unwrap().body, "edited");
    assert_eq!(MessageStore::list_for_channel(s, cid).await.unwrap().len(), 1);

    // ── reactions (dedup + user_has_any) ─────────────────────────────────────
    assert!(ReactionStore::add(s, Reaction::new(mid, uid_b, "👍")).await.unwrap(), "first add sticks");
    assert!(!ReactionStore::add(s, Reaction::new(mid, uid_b, "👍")).await.unwrap(), "dup is a no-op");
    assert!(ReactionStore::user_has_any(s, mid, uid_b).await.unwrap());
    assert_eq!(ReactionStore::list_for_message(s, mid).await.unwrap().len(), 1);
    assert!(ReactionStore::remove(s, mid, uid_b, "👍").await.unwrap());
    assert!(!ReactionStore::user_has_any(s, mid, uid_b).await.unwrap());

    // ── proposals + votes (upsert collapses to one) ──────────────────────────
    let pid = ProposalStore::next_proposal_id(s).await.unwrap();
    let kind = ProposalKind::AddRule { text: "be nice".into() };
    ProposalStore::insert_proposal(s, Proposal::new(pid, sid, uid_a, kind, T, Timestamp(2_000)))
        .await
        .unwrap();
    let mut p = ProposalStore::get_proposal(s, pid).await.unwrap().unwrap();
    p.is_applied = true;
    ProposalStore::update_proposal(s, p).await.unwrap();
    assert!(ProposalStore::get_proposal(s, pid).await.unwrap().unwrap().is_applied);
    assert_eq!(ProposalStore::list_for_server(s, sid).await.unwrap().len(), 1);
    VoteStore::upsert_vote(s, Vote::new(pid, uid_a, true)).await.unwrap();
    VoteStore::upsert_vote(s, Vote::new(pid, uid_a, false)).await.unwrap(); // same voter → overwrite
    assert_eq!(VoteStore::list_for_proposal(s, pid).await.unwrap().len(), 1, "one voter, one row");
    assert!(!VoteStore::get_vote(s, pid, uid_a).await.unwrap().unwrap().is_aye, "the nay won");

    // ── emojis + emoji votes ─────────────────────────────────────────────────
    let eid = EmojiStore::next_emoji_id(s).await.unwrap();
    EmojiStore::insert_emoji(s, Emoji::new(eid, sid, "party", "data:...", uid_a, T)).await.unwrap();
    assert_eq!(EmojiStore::find_emoji(s, sid, "party").await.unwrap().unwrap().id, eid);
    assert_eq!(EmojiStore::list_for_server(s, sid).await.unwrap().len(), 1);
    EmojiVoteStore::upsert_emoji_vote(s, EmojiVote::new(sid, eid, uid_a, true)).await.unwrap();
    assert_eq!(EmojiVoteStore::my_emoji_vote(s, eid, uid_a).await.unwrap(), Some(true));
    assert_eq!(EmojiVoteStore::emoji_votes_for_server(s, sid).await.unwrap().len(), 1);

    // ── rules ────────────────────────────────────────────────────────────────
    let rid = RuleStore::next_rule_id(s).await.unwrap();
    RuleStore::insert_rule(s, Rule::new(rid, sid, "no spam", T)).await.unwrap();
    assert_eq!(RuleStore::list_for_server(s, sid).await.unwrap().len(), 1);
    assert!(RuleStore::remove_rule(s, rid).await.unwrap());
    assert!(RuleStore::list_for_server(s, sid).await.unwrap().is_empty());

    // ── dms (both-direction conversation + partners) ─────────────────────────
    let dm1 = DmStore::next_dm_id(s).await.unwrap();
    DmStore::insert_dm(s, DmMessage::new(dm1, uid_a, uid_b, "toB", "toA", T)).await.unwrap();
    let dm2 = DmStore::next_dm_id(s).await.unwrap();
    DmStore::insert_dm(s, DmMessage::new(dm2, uid_b, uid_a, "toA2", "toB2", Timestamp(1_100)))
        .await
        .unwrap();
    assert_eq!(DmStore::conversation(s, uid_a, uid_b).await.unwrap().len(), 2, "both directions");
    let partners = DmStore::partners(s, uid_a).await.unwrap();
    assert_eq!(partners, vec![uid_b], "grace is ada's sole correspondent");

    // ── blocks ───────────────────────────────────────────────────────────────
    assert!(BlockStore::add(s, Block::new(uid_a, uid_b, T)).await.unwrap());
    assert!(!BlockStore::add(s, Block::new(uid_a, uid_b, T)).await.unwrap(), "dup block no-op");
    assert!(BlockStore::is_blocked_between(s, uid_b, uid_a).await.unwrap(), "symmetric");
    assert_eq!(BlockStore::involving(s, uid_a).await.unwrap().len(), 1);

    // ── friendships ──────────────────────────────────────────────────────────
    FriendStore::add(s, Friendship::request(uid_a, uid_b, T)).await.unwrap();
    let mut fr = FriendStore::between(s, uid_b, uid_a).await.unwrap().unwrap();
    assert!(!fr.are_friends(), "pending");
    fr.accept();
    FriendStore::update(s, fr).await.unwrap();
    assert!(FriendStore::between(s, uid_a, uid_b).await.unwrap().unwrap().are_friends());
    assert_eq!(FriendStore::involving(s, uid_b).await.unwrap().len(), 1);

    // ── roles + assignments ──────────────────────────────────────────────────
    let role_id = RoleStore::next_role_id(s).await.unwrap();
    RoleStore::insert_role(s, Role::new(role_id, sid, "mods", T)).await.unwrap();
    assert_eq!(RoleStore::find_role(s, sid, "mods").await.unwrap().unwrap().id, role_id);
    assert!(RoleStore::assign(s, RoleAssignment::new(sid, role_id, uid_a)).await.unwrap());
    assert!(!RoleStore::assign(s, RoleAssignment::new(sid, role_id, uid_a)).await.unwrap(), "dup");
    assert_eq!(RoleStore::holders(s, role_id).await.unwrap(), vec![uid_a]);
    assert!(RoleStore::unassign(s, role_id, uid_a).await.unwrap());
    assert!(RoleStore::holders(s, role_id).await.unwrap().is_empty());
    assert_eq!(RoleStore::list_for_server(s, sid).await.unwrap().len(), 1);
    assert!(RoleStore::remove_role(s, role_id).await.unwrap());

    // ── role colour votes ────────────────────────────────────────────────────
    let blue = RoleColor::parse("#3b82f6").unwrap();
    RoleColorVoteStore::upsert_role_color_vote(s, RoleColorVote::new(sid, role_id, uid_a, blue.clone()))
        .await
        .unwrap();
    assert_eq!(RoleColorVoteStore::my_role_color_vote(s, role_id, uid_a).await.unwrap(), Some(blue));
    assert_eq!(RoleColorVoteStore::role_color_votes_for_server(s, sid).await.unwrap().len(), 1);

    // ── key directory ────────────────────────────────────────────────────────
    let keys = UserKeys::new(
        uid_a,
        "pubhex",
        WrappedKey { salt: "s".into(), nonce: "n".into(), ciphertext: "c".into() },
    );
    KeyDirectoryStore::put_keys(s, keys).await.unwrap();
    assert_eq!(KeyDirectoryStore::get_keys(s, uid_a).await.unwrap().unwrap().public_key, "pubhex");

    // ── channel key grants ───────────────────────────────────────────────────
    ChannelKeyStore::put_grant(s, ChannelKeyGrant::new(sid, cid, 0, uid_a, "sealed0")).await.unwrap();
    ChannelKeyStore::put_grant(s, ChannelKeyGrant::new(sid, cid, 1, uid_a, "sealed1")).await.unwrap();
    assert_eq!(ChannelKeyStore::grants_for_member(s, cid, uid_a).await.unwrap().len(), 2, "both epochs");

    // ── invites (jsonb_set revoke) ───────────────────────────────────────────
    InviteStore::add(s, Invite::new("hash1", sid, uid_a, T)).await.unwrap();
    assert!(InviteStore::by_hash(s, "hash1").await.unwrap().unwrap().is_live());
    assert_eq!(InviteStore::list_for_server(s, sid).await.unwrap().len(), 1);
    InviteStore::revoke(s, "hash1").await.unwrap();
    assert!(!InviteStore::by_hash(s, "hash1").await.unwrap().unwrap().is_live(), "revoke flipped the flag");
}

/// The rate-cap TOCTOU under real concurrency: with one founding citizen the cap
/// floor gives exactly 5 open slots, so 20 simultaneous admissions of distinct
/// eligible members must admit **exactly 5** — the `SELECT … FOR UPDATE` serializes
/// them so two can never both claim the last slot. Without the lock, more than 5
/// would slip through.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_admissions_never_overrun_the_rate_cap() {
    // A pool wide enough that every racing transaction gets a connection and truly
    // contends on the row lock (not on connection availability).
    let _guard = db_lock().lock().await;
    let Some(store) = fresh_store(24).await else {
        eprintln!("DATABASE_URL unset — skipping the live Postgres rate-cap concurrency test");
        return;
    };
    let s = &*store;

    let founder = UserStore::next_user_id(s).await.unwrap();
    UserStore::insert_user(s, User::new(founder, "founder", T)).await.unwrap();
    let sid = ServerStore::next_server_id(s).await.unwrap();
    ServerStore::insert_server(s, Server::new(sid, "town", "Town", founder, T)).await.unwrap();
    // Seat the founder as the lone citizen. A founder is citizen #1 by founding,
    // not by enfranchisement, so `enfranchised_at` stays None and does not consume a
    // rate-cap slot: 10% of 1 floors to exactly 5 open slots.
    let mut fm = Membership::joined(founder, sid, T);
    fm.tier = Tier::Citizen;
    fm.enfranchised_at = None;
    MembershipStore::upsert(s, fm).await.unwrap();

    // 20 distinct, already-Layer-1-eligible members, each promoted to citizen and
    // ready to be committed iff a slot is open.
    let now = Timestamp(5_000);
    let mut pending = Vec::new();
    for i in 0..20u64 {
        let uid = UserStore::next_user_id(s).await.unwrap();
        UserStore::insert_user(s, User::new(uid, format!("m{i}"), T)).await.unwrap();
        let mut m = Membership::joined(uid, sid, T);
        m.tier = Tier::Citizen;
        m.enfranchised_at = Some(now);
        pending.push(m);
    }

    let window_start = Timestamp(now.0 - 30 * 86_400);
    let mut tasks = Vec::new();
    for m in pending {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            MembershipStore::admit_within_cap(&*store, m, window_start, &enfranchisement_slots)
                .await
                .unwrap()
        }));
    }
    let mut admitted = 0u64;
    for t in tasks {
        if matches!(t.await.unwrap(), app::CapAdmission::Admitted) {
            admitted += 1;
        }
    }

    assert_eq!(admitted, 5, "exactly the 5 open slots were filled, never more");
    assert_eq!(
        MembershipStore::citizen_count(s, sid).await.unwrap(),
        6,
        "founder + 5 admitted; the lock held the cap under 20-way contention",
    );
}

/// The transactional outbox producer: writes made through the ordinary ports must
/// appear in `changes_since` in commit order with the right entity/op, and
/// `sign_feed` must turn that outbox into a valid signed feed straight from the
/// PgStore — proving it is a drop-in `ChangeSource` for federation.
#[tokio::test]
async fn writes_land_in_the_transactional_outbox_and_sign() {
    use federation::{
        sign_feed, ChangeOp, ChangeSource, InMemoryRegistry, NodeKeypair, OwnedScope,
        OwnershipRegistry, ScopeResolver,
    };

    struct NoParents;
    #[async_trait::async_trait]
    impl ScopeResolver for NoParents {
        async fn proposal_server(&self, _: u64) -> Option<u64> {
            None
        }
        async fn message_server(&self, _: u64) -> Option<u64> {
            None
        }
    }

    let _guard = db_lock().lock().await;
    let Some(store) = fresh_store(5).await else {
        eprintln!("DATABASE_URL unset — skipping the live Postgres outbox test");
        return;
    };
    let s = &*store;

    // A server, a channel, and a message — three writes, three outbox rows.
    let founder = UserStore::next_user_id(s).await.unwrap();
    UserStore::insert_user(s, User::new(founder, "ada", T)).await.unwrap();
    let sid = domain::ServerId(7);
    ServerStore::insert_server(s, Server::new(sid, "town", "Town", founder, T)).await.unwrap();
    let cid = ChannelStore::next_channel_id(s).await.unwrap();
    ChannelStore::insert_channel(s, Channel::new(cid, sid, "general", "", T)).await.unwrap();
    let mid = MessageStore::next_message_id(s).await.unwrap();
    MessageStore::insert_message(s, Message::new(mid, cid, sid, founder, "hi", None, T)).await.unwrap();

    let feed = ChangeSource::changes_since(s, 0, 100).await;
    let entities: Vec<&str> = feed.iter().map(|r| r.entity.as_str()).collect();
    assert_eq!(entities, ["users", "servers", "channels", "messages"], "in commit order");
    let seqs: Vec<u64> = feed.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4], "monotonic outbox sequence");
    assert!(feed.iter().all(|r| r.op == ChangeOp::Upsert));

    // A cursor pull returns only the tail.
    let tail = ChangeSource::changes_since(s, 2, 100).await;
    assert_eq!(tail.len(), 2, "a peer at cursor 2 sees only the channel + message");

    // Sign the outbox straight from the PgStore: node A owns Server(7) + the user's
    // home, so its rows sign; every event verifies under A's key.
    let node_a = NodeKeypair::generate(domain::NodeId(1));
    let reg = InMemoryRegistry::new();
    reg.publish_key(domain::NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), domain::NodeId(1)).await.unwrap();
    reg.claim(OwnedScope::UserHome(founder.0), domain::NodeId(1)).await.unwrap();
    let signed = sign_feed(s, &node_a, &reg, &NoParents, 0, 100).await;
    assert_eq!(signed.len(), 4, "all four owned rows signed");
    assert!(signed.iter().all(|e| e.verify(&node_a.public()).is_ok()), "A signs its own feed");
}
