//! The use-cases — the single entry point a driving adapter (CLI, web) calls.
//!
//! Every method is a thin orchestration over the pure `domain` rules and the
//! `ports`: no business rule lives here that isn't ultimately a domain function.

use std::sync::Arc;

use domain::{
    enfranchisement_slots, evaluate_eligibility, slugify, Eligibility, Invite, Server, Membership,
    Phase, Tier, Timestamp, User,
};

use crate::CapAdmission;
use crate::channel_key_service::ChannelKeyService;
use crate::chat_service::ChatService;
use crate::emoji_service::EmojiService;
use crate::governance_service::GovernanceService;
use crate::key_directory_service::KeyDirectoryService;
use crate::mute_service::MuteService;
use crate::outcome::EnfranchiseOutcome;
use crate::role_service::RoleService;
use crate::social_service::SocialService;
use crate::stores::Stores;
use crate::{
    ChannelStore, Clock, EnfranchiseError, FoundError, InviteError, InviteStore, JoinError,
    MembershipStore, RegisterError, ServerStore, UserStore,
};

/// The trailing window the enfranchisement rate cap measures admissions over.
const RATE_CAP_WINDOW_DAYS: i64 = 30;

/// The single entry point a driving adapter calls. Holds the "core" identity,
/// server, membership, invite, and enfranchise use-cases directly, and exposes
/// the per-domain use-cases (chat, governance, social, …) through accessor
/// handles that return the relevant sub-service.
#[derive(Clone)]
pub struct Services {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) users: Arc<dyn UserStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) channels: Arc<dyn ChannelStore>,
    pub(crate) invites: Arc<dyn InviteStore>,
    chat: ChatService,
    governance: GovernanceService,
    social: SocialService,
    roles: RoleService,
    emoji: EmojiService,
    mute: MuteService,
    channel_keys: ChannelKeyService,
    keys: KeyDirectoryService,
}

impl Services {
    pub fn new(clock: Arc<dyn Clock>, stores: Stores) -> Self {
        let chat = ChatService {
            channels: stores.channels.clone(),
            clock: clock.clone(),
            image: stores.image.clone(),
            media: stores.media.clone(),
            memberships: stores.memberships.clone(),
            messages: stores.messages.clone(),
            reactions: stores.reactions.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let governance = GovernanceService {
            channels: stores.channels.clone(),
            clock: clock.clone(),
            emojis: stores.emojis.clone(),
            memberships: stores.memberships.clone(),
            proposals: stores.proposals.clone(),
            roles: stores.roles.clone(),
            rules: stores.rules.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
            votes: stores.votes.clone(),
        };
        let social = SocialService {
            blocks: stores.blocks.clone(),
            clock: clock.clone(),
            dms: stores.dms.clone(),
            friends: stores.friends.clone(),
            users: stores.users.clone(),
        };
        let roles = RoleService {
            memberships: stores.memberships.clone(),
            role_color_votes: stores.role_color_votes.clone(),
            roles: stores.roles.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let emoji = EmojiService {
            clock: clock.clone(),
            emoji_votes: stores.emoji_votes.clone(),
            emojis: stores.emojis.clone(),
            memberships: stores.memberships.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let mute = MuteService {
            clock: clock.clone(),
            memberships: stores.memberships.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let channel_keys = ChannelKeyService {
            channel_keys: stores.channel_keys.clone(),
            channels: stores.channels.clone(),
            memberships: stores.memberships.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let keys = KeyDirectoryService {
            keys: stores.keys.clone(),
            users: stores.users.clone(),
        };
        Self {
            clock,
            users: stores.users,
            servers: stores.servers,
            memberships: stores.memberships,
            channels: stores.channels,
            invites: stores.invites,
            chat,
            governance,
            social,
            roles,
            emoji,
            mute,
            channel_keys,
            keys,
        }
    }

    /// Chat use-cases: channels, messages, threaded replies, reactions, and media.
    pub fn chat(&self) -> &ChatService {
        &self.chat
    }

    /// Governance use-cases: proposals, voting, deliberation, and effect application.
    pub fn governance(&self) -> &GovernanceService {
        &self.governance
    }

    /// Social use-cases: direct messages, blocks, and friendships.
    pub fn social(&self) -> &SocialService {
        &self.social
    }

    /// Role use-cases: custom roles, their holders/colours, and `@mention` resolution.
    pub fn roles(&self) -> &RoleService {
        &self.roles
    }

    /// Custom-emoji use-cases: add, vote, and the ranked list.
    pub fn emoji(&self) -> &EmojiService {
        &self.emoji
    }

    /// Policing use-cases: instant mute/unmute and the moderation read-side.
    pub fn mute(&self) -> &MuteService {
        &self.mute
    }

    /// Encrypted-channel use-cases: enable encryption and channel-key grants.
    pub fn channel_keys(&self) -> &ChannelKeyService {
        &self.channel_keys
    }

    /// Key-directory use-cases: publish and fetch device keys.
    pub fn keys(&self) -> &KeyDirectoryService {
        &self.keys
    }

    /// Register a new platform account **without** a password. For seeding, the
    /// CLI, and tests only — such an account cannot authenticate to the web app
    /// ([`authenticate`](Self::authenticate) rejects a passwordless account) until
    /// a password is set. Web sign-up uses
    /// [`register_with_password`](Self::register_with_password).
    pub async fn register_account(&self, handle: &str) -> Result<User, RegisterError> {
        let handle = handle.trim();
        if handle.is_empty() {
            return Err(RegisterError::EmptyHandle);
        }
        if self.users.find_by_handle(handle).await?.is_some() {
            return Err(RegisterError::HandleTaken(handle.to_string()));
        }
        let user = User::new(self.users.next_user_id().await?, handle, self.clock.now());
        self.users.insert_user(user.clone()).await?;
        Ok(user)
    }

    /// Register a new account with a password. The password is length-validated by
    /// the domain and hashed with Argon2id before storage; the plaintext never
    /// persists. This is the only registration path the web signup uses.
    pub async fn register_with_password(
        &self,
        handle: &str,
        password: &str,
    ) -> Result<User, RegisterError> {
        let handle = handle.trim();
        if handle.is_empty() {
            return Err(RegisterError::EmptyHandle);
        }
        domain::validate_password(password)
            .map_err(|e| RegisterError::WeakPassword(e.to_string()))?;
        if self.users.find_by_handle(handle).await?.is_some() {
            return Err(RegisterError::HandleTaken(handle.to_string()));
        }
        let hash = crate::hash_password(password).map_err(|_| RegisterError::HashFailed)?;
        let mut user = User::new(self.users.next_user_id().await?, handle, self.clock.now());
        user.password_hash = hash;
        self.users.insert_user(user.clone()).await?;
        Ok(user)
    }

    /// Set (or replace) an account's password. Used to give seed/CLI accounts a
    /// real credential so the demo can log in as them.
    pub async fn set_password(&self, handle: &str, password: &str) -> Result<(), RegisterError> {
        domain::validate_password(password)
            .map_err(|e| RegisterError::WeakPassword(e.to_string()))?;
        let mut user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or(RegisterError::EmptyHandle)?;
        user.password_hash = crate::hash_password(password).map_err(|_| RegisterError::HashFailed)?;
        self.users.update_user(user).await?;
        Ok(())
    }

    /// Authenticate a handle + password, returning the account on success and
    /// `None` on any failure (unknown handle, passwordless account, or wrong
    /// password). Every failure path spends one Argon2 verify's worth of time
    /// ([`spend_verify_time`](crate::spend_verify_time)) so an attacker cannot
    /// tell "no such account" from "wrong password" by timing.
    pub async fn authenticate(&self, handle: &str, password: &str) -> Option<User> {
        match self.users.find_by_handle(handle.trim()).await.ok().flatten() {
            Some(user) if user.has_password() => {
                if crate::verify_password(password, &user.password_hash) {
                    Some(user)
                } else {
                    None
                }
            }
            // Unknown handle or passwordless account: burn equivalent time.
            _ => {
                crate::spend_verify_time();
                None
            }
        }
    }

    /// Found a new server. The founder becomes **citizen #1** — the single
    /// structural bootstrap in the whole system, justified because a server of one
    /// needs a first citizen to exist at all. This influence is *diluted*
    /// automatically as the server grows across phases; it is not founder-for-life,
    /// and it is not a power to enfranchise anyone *else* (which stays
    /// criteria-only). A franchise-barred puppet account may not found a server.
    pub async fn found_server(
        &self,
        founder_handle: &str,
        name: &str,
    ) -> Result<Server, FoundError> {
        self.found_server_with_visibility(founder_handle, name, false).await
    }

    /// Found a server, choosing its visibility. A **private** server is hidden from
    /// the public browse directory and joinable only with an invite code; a public
    /// one is listed and freely joinable. Either way the founder becomes citizen #1
    /// and the invite policy starts [`InvitePolicy::Open`](domain::InvitePolicy) so
    /// the community can grow toward its first ten voters.
    pub async fn found_server_with_visibility(
        &self,
        founder_handle: &str,
        name: &str,
        is_private: bool,
    ) -> Result<Server, FoundError> {
        let founder = self
            .users
            .find_by_handle(founder_handle.trim()).await?
            .ok_or_else(|| FoundError::NoSuchUser(founder_handle.to_string()))?;
        if founder.is_franchise_barred {
            return Err(FoundError::FounderBarred);
        }
        let slug = slugify(name);
        if slug.is_empty() {
            return Err(FoundError::EmptyName);
        }
        if self.servers.find_by_slug(&slug).await?.is_some() {
            return Err(FoundError::SlugTaken(slug));
        }

        let now = self.clock.now();
        let mut server =
            Server::new(self.servers.next_server_id().await?, slug, name.trim(), founder.id, now);
        server.is_private = is_private;
        self.servers.insert_server(server.clone()).await?;

        // Founder joins as citizen #1.
        let mut m = Membership::joined(founder.id, server.id, now);
        m.tier = Tier::Citizen;
        m.enfranchised_at = Some(now);
        self.memberships.upsert(m).await?;

        // Every server starts with #general (somewhere to post from the first
        // moment) and the restricted #appeals room. Past Seed, further channels are
        // governed by ballot; these two are the provisioning floor no server is
        // ever without.
        self.ensure_default_channels(server.id, now).await;

        Ok(server)
    }

    /// Ensure a server has its floor channels — a plaintext `#general` and the
    /// restricted `#appeals` — creating whichever is missing. Idempotent: a server
    /// that already has them is untouched. Used at founding and by
    /// [`backfill_default_channels`](Self::backfill_default_channels).
    async fn ensure_default_channels(&self, sid: domain::ServerId, now: Timestamp) {
        let general = domain::normalize_channel_name("general");
        if self.channels.find_by_name(sid, &general).await.ok().flatten().is_none() {
            self.channels.insert_channel(domain::Channel::new(
                self.channels.next_channel_id().await.unwrap_or_default(),
                sid,
                general,
                "",
                now,
            )).await.unwrap_or_default();
        }
        let appeals = domain::normalize_channel_name("appeals");
        if self.channels.find_by_name(sid, &appeals).await.ok().flatten().is_none() {
            self.channels.insert_channel(domain::Channel::appeals(
                self.channels.next_channel_id().await.unwrap_or_default(),
                sid,
                appeals,
                "Appeal a mute here — visible to voters and police.",
                now,
            )).await.unwrap_or_default();
        }
    }

    /// Backfill the floor channels for **every** server. Older datasets hold
    /// servers founded before channels were auto-provisioned (and before #appeals
    /// existed at all); running this once at boot gives each of them a #general and
    /// #appeals. Idempotent — safe to run on every start.
    pub async fn backfill_default_channels(&self) {
        let now = self.clock.now();
        for server in self.servers.list_all().await.unwrap_or_default() {
            self.ensure_default_channels(server.id, now).await;
        }
    }

    /// Mint an invite code for a server, returning the **raw** code to share (only
    /// its digest is stored). Gated on: the actor is a member, and the server's
    /// [`InvitePolicy`](domain::InvitePolicy) is `Open`. An invite grants membership
    /// only — never the franchise.
    pub async fn create_invite(&self, handle: &str, server_slug: &str) -> Result<String, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).await?.is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        if !server.allows_member_invites() {
            return Err(InviteError::Closed(server_slug.to_string()));
        }
        let code = crate::invite::new_invite_code::new_invite_code();
        let hash = crate::invite::hash_code::hash_code(&code);
        self.invites
            .add(Invite::new(hash, server.id, user.id, self.clock.now())).await?;
        Ok(code)
    }

    /// Redeem an invite code: join its server as an ordinary member. Fails if the
    /// code is unknown/revoked, the server has since closed invites, or the redeemer
    /// already belongs. Never grants citizenship.
    pub async fn accept_invite(&self, handle: &str, code: &str) -> Result<Membership, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let hash = crate::invite::hash_code::hash_code(code);
        let invite = self
            .invites
            .by_hash(&hash).await?
            .filter(Invite::is_live)
            .ok_or(InviteError::InvalidCode)?;
        let server = self
            .servers
            .get_server(invite.server_id).await?
            .ok_or(InviteError::InvalidCode)?;
        if !server.allows_member_invites() {
            return Err(InviteError::Closed(server.slug.clone()));
        }
        if self.memberships.get(user.id, server.id).await?.is_some() {
            return Err(InviteError::AlreadyMember(server.slug));
        }
        let m = Membership::joined(user.id, server.id, self.clock.now());
        self.memberships.upsert(m.clone()).await?;
        Ok(m)
    }

    /// The live invites for a server, for a member to view/share. Members only.
    pub async fn list_invites(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<Vec<Invite>, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).await?.is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        Ok(self
            .invites
            .list_for_server(server.id).await?
            .into_iter()
            .filter(Invite::is_live)
            .collect())
    }

    /// Revoke one of a server's invites by its code digest. Members only.
    pub async fn revoke_invite(
        &self,
        handle: &str,
        server_slug: &str,
        code_hash: &str,
    ) -> Result<(), InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).await?.is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        // Only revoke a hash that actually belongs to this server.
        if self
            .invites
            .by_hash(code_hash).await?
            .is_some_and(|i| i.server_id == server.id)
        {
            self.invites.revoke(code_hash).await?;
        }
        Ok(())
    }

    /// Join a server as an ordinary member (tier `Member`). Joining accrues dwell
    /// time toward the franchise but confers **no** vote.
    pub async fn join_server(&self, handle: &str, server_slug: &str) -> Result<Membership, JoinError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| JoinError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| JoinError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).await?.is_some() {
            return Err(JoinError::AlreadyMember(server_slug.to_string()));
        }
        let m = Membership::joined(user.id, server.id, self.clock.now());
        self.memberships.upsert(m.clone()).await?;
        Ok(m)
    }

    /// Attempt to enfranchise a member. This is the **only** path by which a
    /// non-founder becomes a citizen, and it runs entirely on the domain rules:
    /// Layer 1 (are the criteria met?) then Layer 2 (is a rate-cap slot open?).
    /// There is no override.
    pub async fn try_enfranchise(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<EnfranchiseOutcome, EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let mut membership = self
            .memberships
            .get(user.id, server.id).await?
            .ok_or_else(|| EnfranchiseError::NotAMember(handle.to_string()))?;

        if membership.is_citizen() {
            return Err(EnfranchiseError::AlreadyCitizen(handle.to_string()));
        }

        let now = self.clock.now();

        // Layer 1 — earned franchise.
        let eligibility = evaluate_eligibility(&user, &membership, &server.criteria, now);
        if !eligibility.is_eligible() {
            return Ok(EnfranchiseOutcome::NotEligible(eligibility.unmet));
        }

        // Layer 2 — enfranchisement rate cap. The count-then-admit runs as one
        // atomic, server-locked step in the store so two concurrent admissions can't
        // both claim the final slot; the domain rule rides along as `slots_open`.
        membership.tier = Tier::Citizen;
        membership.enfranchised_at = Some(now);
        let window_start = Timestamp(now.0 - RATE_CAP_WINDOW_DAYS * Timestamp::SECONDS_PER_DAY);
        match self
            .memberships
            .admit_within_cap(membership, window_start, &enfranchisement_slots)
            .await?
        {
            CapAdmission::Admitted => Ok(EnfranchiseOutcome::Admitted),
            CapAdmission::RateCapped { admitted_this_window } => {
                Ok(EnfranchiseOutcome::RateCapped { admitted_this_window })
            }
        }
    }

    /// Read-only: how a member currently stands against the franchise criteria.
    /// Powers a "why can't I vote yet?" view without any side effect.
    pub async fn eligibility(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<Eligibility, EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let membership = self
            .memberships
            .get(user.id, server.id).await?
            .ok_or_else(|| EnfranchiseError::NotAMember(handle.to_string()))?;
        Ok(evaluate_eligibility(&user, &membership, &server.criteria, self.clock.now()))
    }

    /// Read-only: every server plus its phase and citizen count, for a directory.
    pub async fn list_servers(&self) -> Vec<(Server, Phase, u64)> {
        let mut out = Vec::new();
        for g in self.servers.list_all().await.unwrap_or_default() {
            let citizens = self.memberships.citizen_count(g.id).await.unwrap_or_default();
            let phase = Phase::from_citizen_count(citizens);
            out.push((g, phase, citizens));
        }
        out
    }

    /// The servers this user belongs to — powers their sidebar. A user only ever
    /// sees servers they've joined (public or private); discovery of *new* servers
    /// is [`list_public_servers`](Self::list_public_servers).
    pub async fn my_servers(&self, handle: &str) -> Vec<(Server, Phase, u64)> {
        let Some(user) = self.users.find_by_handle(handle.trim()).await.ok().flatten() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (g, phase, citizens) in self.list_servers().await {
            if self.memberships.get(user.id, g.id).await.ok().flatten().is_some() {
                out.push((g, phase, citizens));
            }
        }
        out
    }

    /// The slug of a server by id, if it exists (for turning a redeemed invite's
    /// server id back into a URL the client can open).
    pub async fn server_slug(&self, id: domain::ServerId) -> Option<String> {
        self.servers.get_server(id).await.ok().flatten().map(|g| g.slug)
    }

    /// The public browse directory: every **public** server. Private servers are
    /// omitted — they are reachable only by invite code.
    pub async fn list_public_servers(&self) -> Vec<(Server, Phase, u64)> {
        self.list_servers()
            .await
            .into_iter()
            .filter(|(g, _, _)| !g.is_private)
            .collect()
    }

    /// Read-only: look up an account by handle.
    pub async fn find_user(&self, handle: &str) -> Option<User> {
        self.users.find_by_handle(handle.trim()).await.ok().flatten()
    }

    /// The current instant according to the injected clock.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// Read-only: a member's tier within a server, if they belong.
    pub async fn member_tier(&self, handle: &str, server_slug: &str) -> Option<Tier> {
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        self.memberships.get(user.id, server.id).await.ok().flatten().map(|m| m.tier)
    }

    /// Read-only: a member's endorsement-weighted contribution in a server.
    pub async fn member_contribution(&self, handle: &str, server_slug: &str) -> Option<i64> {
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        self.memberships.get(user.id, server.id).await.ok().flatten().map(|m| m.contribution)
    }

    /// Read-only snapshot of a server: the server, its current phase, and its
    /// citizen count.
    pub async fn server_snapshot(&self, server_slug: &str) -> Option<(Server, Phase, u64)> {
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        let citizens = self.memberships.citizen_count(server.id).await.unwrap_or_default();
        Some((server.clone(), Phase::from_citizen_count(citizens), citizens))
    }

    /// Dev/testing helper: set a member's endorsement-weighted contribution
    /// directly. In the real system this score is produced by citizens reacting
    /// positively to a member's messages; here it lets the CLI demonstrate the
    /// franchise criteria without a full reaction pipeline. It changes *only* the
    /// contribution input to Layer 1 — it never enfranchises anyone.
    pub async fn set_contribution(
        &self,
        handle: &str,
        server_slug: &str,
        contribution: i64,
    ) -> Result<(), EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let mut membership = self
            .memberships
            .get(user.id, server.id).await?
            .ok_or_else(|| EnfranchiseError::NotAMember(handle.to_string()))?;
        membership.contribution = contribution;
        self.memberships.upsert(membership).await?;
        Ok(())
    }
}
