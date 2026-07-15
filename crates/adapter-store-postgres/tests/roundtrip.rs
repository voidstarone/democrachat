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
    Block, Channel, ChannelKeyGrant, DmMessage, Emoji, EmojiVote, Friendship, Invite, Membership,
    Message, Proposal, ProposalKind, Reaction, Role, RoleAssignment, RoleColor, RoleColorVote,
    Rule, Server, Tier, Timestamp, User, UserKeys, Vote, WrappedKey,
};

const T: Timestamp = Timestamp(1_000);

/// Wipe and rebuild the schema so each run starts clean, then connect (which runs
/// the DDL). Returns `None` when `DATABASE_URL` is unset — the caller then skips.
async fn fresh_store() -> Option<std::sync::Arc<PgStore>> {
    let url = std::env::var("DATABASE_URL").ok()?;
    let reset = sqlx::postgres::PgPool::connect(&url).await.expect("connect for reset");
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(&reset)
        .await
        .expect("reset schema");
    reset.close().await;
    Some(PgStore::connect(&url, 5).await.expect("connect PgStore"))
}

#[tokio::test]
async fn every_port_round_trips_against_a_live_postgres() {
    let Some(store) = fresh_store().await else {
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
