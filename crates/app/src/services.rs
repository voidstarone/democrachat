//! The use-cases — the single entry point a driving adapter (CLI, web) calls.
//!
//! Every method is a thin orchestration over the pure `domain` rules and the
//! `ports`: no business rule lives here that isn't ultimately a domain function.

use std::sync::Arc;

use domain::{
    enfranchisement_slots, evaluate_eligibility, slugify, Eligibility, Invite, Server, Membership,
    Phase, PhaseThresholds, Tier, Timestamp, User,
};

use crate::CapAdmission;
use crate::channel_key_service::ChannelKeyService;
use crate::chat_service::ChatService;
use crate::emoji_service::EmojiService;
use crate::governance_service::GovernanceService;
use crate::key_directory_service::KeyDirectoryService;
use crate::mute_service::MuteService;
use crate::outcome::{EnfranchiseOutcome, TrustedFranchise};
use crate::role_service::RoleService;
use crate::social_service::SocialService;
use crate::tag_service::TagService;
use crate::stores::Stores;
use crate::{
    ChannelStore, Clock, EmailVerificationMode, EnfranchiseError, FoundError, InviteError,
    InviteStore, JoinError, MembershipStore, RegisterError, ServerStore, UserStore, VaultKey,
    VerificationTokenStore,
};

/// The trailing window the enfranchisement rate cap measures admissions over.
const RATE_CAP_WINDOW_DAYS: i64 = 30;

/// How long an emailed verification token stays valid (24h, in seconds).
const VERIFICATION_TTL_SECS: i64 = 24 * 60 * 60;

/// The result of a successful web registration.
#[derive(Debug)]
pub struct Registration {
    /// The newly-created account.
    pub user: User,
    /// The raw verification token to email, when the deployment requires
    /// verification (and an email key is configured). `None` when verification is
    /// off — the account is already usable and no email need be sent.
    pub verification_token: Option<String>,
}

/// What the web layer needs to re-send a verification email: the decrypted address
/// and a freshly-issued raw token.
#[derive(Debug)]
pub struct ResendTarget {
    pub email: String,
    pub token: String,
}

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
    pub(crate) verification_tokens: Arc<dyn VerificationTokenStore>,
    /// Verification policy (default [`EmailVerificationMode::Off`]); set by the
    /// composition root via [`with_email_policy`](Self::with_email_policy).
    pub(crate) email_verification: EmailVerificationMode,
    /// The dedicated key emails are sealed under, or `None` when no address is
    /// collected/stored (off mode / CLI / tests).
    pub(crate) email_key: Option<VaultKey>,
    /// How many days a founding member's vote stands before it must be backed by a
    /// confirmed address. Operator policy; see
    /// [`with_founding_grace_days`](Self::with_founding_grace_days).
    pub(crate) founding_grace_days: i64,
    /// Where this deployment's bootstrap phases begin. Operator policy; see
    /// [`with_phase_thresholds`](Self::with_phase_thresholds).
    pub(crate) phase_thresholds: PhaseThresholds,
    chat: ChatService,
    governance: GovernanceService,
    social: SocialService,
    roles: RoleService,
    emoji: EmojiService,
    mute: MuteService,
    channel_keys: ChannelKeyService,
    keys: KeyDirectoryService,
    tags: TagService,
}

impl Services {
    pub fn new(clock: Arc<dyn Clock>, stores: Stores) -> Self {
        let chat = ChatService {
            channels: stores.channels.clone(),
            clock: clock.clone(),
            phase_thresholds: PhaseThresholds::platform_default(),
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
            phase_thresholds: PhaseThresholds::platform_default(),
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
            clock: clock.clone(),
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
            clock: clock.clone(),
            memberships: stores.memberships.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        let keys = KeyDirectoryService {
            keys: stores.keys.clone(),
            users: stores.users.clone(),
        };
        let tags = TagService {
            channels: stores.channels.clone(),
            servers: stores.servers.clone(),
            users: stores.users.clone(),
        };
        Self {
            clock,
            users: stores.users,
            servers: stores.servers,
            memberships: stores.memberships,
            channels: stores.channels,
            invites: stores.invites,
            verification_tokens: stores.verification_tokens,
            email_verification: EmailVerificationMode::Off,
            email_key: None,
            founding_grace_days: domain::DEFAULT_UNCONFIRMED_FRANCHISE_GRACE_DAYS,
            phase_thresholds: PhaseThresholds::platform_default(),
            chat,
            governance,
            social,
            roles,
            emoji,
            mute,
            channel_keys,
            keys,
            tags,
        }
    }

    /// Configure the email-verification policy. The composition root calls this
    /// with the mode resolved from `DEMOCRACHAT_EMAIL_VERIFICATION` and the key
    /// from `DEMOCRACHAT_EMAIL_KEY`. Left at the default (`Off`, no key),
    /// registration stores no email and every account is immediately usable —
    /// which is exactly what the CLI and the test fixtures want.
    pub fn with_email_policy(mut self, mode: EmailVerificationMode, key: Option<VaultKey>) -> Self {
        self.email_verification = mode;
        self.email_key = key;
        self
    }

    /// Set how long a founding member's vote stands before it must be backed by a
    /// confirmed email address (`DEMOCRACHAT_FRANCHISE_GRACE_DAYS`). `0` keeps the
    /// founding head start but seats nobody unconfirmed. Only meaningful in a mode
    /// that asks for confirmation at all.
    pub fn with_founding_grace_days(mut self, days: i64) -> Self {
        self.founding_grace_days = days.max(0);
        self
    }

    /// Set where this deployment's bootstrap phases begin
    /// (`DEMOCRACHAT_CHARTERING_AT` / `DEMOCRACHAT_SOVEREIGN_AT`). Propagated to the
    /// sub-services that judge phase themselves, so a configured deployment cannot
    /// end up with one service still using the platform default.
    pub fn with_phase_thresholds(mut self, thresholds: PhaseThresholds) -> Self {
        self.phase_thresholds = thresholds;
        self.chat.phase_thresholds = thresholds;
        self.governance.phase_thresholds = thresholds;
        self
    }

    /// The deployment's email-verification policy, for the web layer's login gate.
    pub fn email_verification(&self) -> EmailVerificationMode {
        self.email_verification
    }

    /// The franchise half of that policy, handed to every
    /// [`evaluate_eligibility`] call so an unconfirmed address is judged in the
    /// one place allowed to conclude "eligible" — and never forgotten at a call
    /// site.
    fn franchise_email_rule(&self) -> domain::EmailFranchiseRule {
        self.email_verification.franchise_rule(self.founding_grace_days)
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

    /// Tag use-cases: label servers/channels/users and discover by tag.
    pub fn tags(&self) -> &TagService {
        &self.tags
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
        let mut user = User::new(self.users.next_user_id().await?, handle, self.clock.now());
        // Seed/CLI accounts carry no email and are exempt from the verification
        // gate, so they remain usable even in hard mode.
        user.email_verified = true;
        self.users.insert_user(user.clone()).await?;
        Ok(user)
    }

    /// Register a new account with a password + email. The password is
    /// length-validated and Argon2id-hashed; the email is format-validated, then —
    /// when an email key is configured — stored **only encrypted** (never
    /// plaintext) after a decrypt-and-compare uniqueness check. This is the web
    /// signup path.
    ///
    /// In hard mode the account is created **unverified** and a raw verification
    /// token is returned in [`Registration::verification_token`] for the web layer
    /// to email; in off mode (or with no key) the account is created already
    /// verified and no token is issued.
    pub async fn register_with_password(
        &self,
        handle: &str,
        email: &str,
        password: &str,
    ) -> Result<Registration, RegisterError> {
        let handle = handle.trim();
        if handle.is_empty() {
            return Err(RegisterError::EmptyHandle);
        }
        domain::validate_password(password)
            .map_err(|e| RegisterError::WeakPassword(e.to_string()))?;
        domain::validate_email(email)
            .map_err(|e| RegisterError::InvalidEmail(e.to_string()))?;
        if self.users.find_by_handle(handle).await?.is_some() {
            return Err(RegisterError::HandleTaken(handle.to_string()));
        }

        // Emails are persisted only as ciphertext. With a key configured, enforce
        // uniqueness by decrypting each stored address and comparing (no
        // deterministic index is stored). With no key (off mode / CLI), the
        // address is simply not persisted — never in plaintext.
        let email_enc = if let Some(key) = &self.email_key {
            let normalized = crate::email::normalize_email(email);
            let clash = self
                .users
                .list_all()
                .await?
                .into_iter()
                .filter(|u| u.has_email())
                .any(|u| {
                    crate::email::open_email(key, &u.email_enc)
                        .map(|e| crate::email::normalize_email(&e) == normalized)
                        .unwrap_or(false)
                });
            if clash {
                return Err(RegisterError::EmailTaken);
            }
            crate::email::seal_email(key, email.trim())
        } else {
            String::new()
        };

        let hash = crate::hash_password(password).map_err(|_| RegisterError::HashFailed)?;
        let mut user = User::new(self.users.next_user_id().await?, handle, self.clock.now());
        user.password_hash = hash;
        user.email_enc = email_enc;

        let verification_token =
            if self.email_verification.issues_verification() && self.email_key.is_some() {
                user.email_verified = false;
                let raw = crate::email::new_verification_token();
                let expires_at = self.clock.now().0 + VERIFICATION_TTL_SECS;
                self.verification_tokens
                    .add(crate::email::hash_token(&raw), user.id, expires_at)
                    .await?;
                Some(raw)
            } else {
                user.email_verified = true;
                None
            };

        self.users.insert_user(user.clone()).await?;
        Ok(Registration { user, verification_token })
    }

    /// Consume a verification token, marking its account's email verified. Returns
    /// the updated account, or `None` if the token is unknown, already used, or
    /// expired. Single-use.
    pub async fn verify_email(&self, raw_token: &str) -> Result<Option<User>, RegisterError> {
        let now = self.clock.now().0;
        let Some(user_id) = self
            .verification_tokens
            .take(&crate::email::hash_token(raw_token), now)
            .await?
        else {
            return Ok(None);
        };
        let Some(mut user) = self.users.get_user(user_id).await? else {
            return Ok(None);
        };
        user.email_verified = true;
        self.users.update_user(user.clone()).await?;
        Ok(Some(user))
    }

    /// Re-issue a verification token for an as-yet-unverified account, returning the
    /// decrypted address and raw token for the web layer to email. `None` for every
    /// "nothing to do" case (verification off / no key, unknown handle, already
    /// verified, or no email on file) — the web layer maps them all to one opaque
    /// response so this is not an account-existence oracle.
    pub async fn issue_resend(&self, handle: &str) -> Result<Option<ResendTarget>, RegisterError> {
        let Some(key) = self.email_key.as_ref() else {
            return Ok(None);
        };
        // Any mode that issues links must also re-issue them: under soft
        // verification the first link often expires unused (nothing is blocked, so
        // there is no urgency), and the member only comes looking for another once
        // the franchise is in reach — a month or more later.
        if !self.email_verification.issues_verification() {
            return Ok(None);
        }
        let Some(user) = self.users.find_by_handle(handle.trim()).await? else {
            return Ok(None);
        };
        if user.email_verified || !user.has_email() {
            return Ok(None);
        }
        let Ok(email) = crate::email::open_email(key, &user.email_enc) else {
            return Ok(None);
        };
        let raw = crate::email::new_verification_token();
        let expires_at = self.clock.now().0 + VERIFICATION_TTL_SECS;
        self.verification_tokens
            .add(crate::email::hash_token(&raw), user.id, expires_at)
            .await?;
        Ok(Some(ResendTarget { email, token: raw }))
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
    /// the community can grow toward its first few voters.
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

        // Founder joins as citizen #1 — the one seat that never runs through
        // `evaluate_eligibility`, since there is no electorate yet to qualify
        // against. Founding a server does not wait on an email either: an
        // unconfirmed founder votes on trust, with the same deadline their founding
        // cohort gets.
        let mut m = Membership::joined(founder.id, server.id, now);
        let rule = self.franchise_email_rule();
        let needs_trust = rule.requires_confirmation() && !founder.is_email_verified();
        if !needs_trust || rule.extends_founding_trust() {
            m.tier = Tier::Citizen;
            m.enfranchised_at = Some(now);
            if needs_trust {
                m.unconfirmed_franchise_until =
                    Some(domain::confirmation_deadline(now, rule.founding_grace_days()));
            }
        }
        // With a grace of zero an unconfirmed founder joins as a plain member and is
        // seated the moment they confirm — the automatic sweep sees a Seed member
        // whose only bar has just cleared.
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
        let phase = self.phase_of(server.id).await;

        // Layer 1 — earned franchise.
        let eligibility = evaluate_eligibility(
            &user,
            &membership,
            &server.criteria,
            phase,
            self.franchise_email_rule(),
            now,
        );
        if !eligibility.is_eligible() {
            return Ok(EnfranchiseOutcome::NotEligible(eligibility.unmet));
        }

        // Layer 2 — enfranchisement rate cap. The count-then-admit runs as one
        // atomic, server-locked step in the store so two concurrent admissions can't
        // both claim the final slot; the domain rule rides along as `slots_open`.
        membership.tier = Tier::Citizen;
        membership.enfranchised_at = Some(now);
        if !user.is_email_verified() {
            // Eligible while unconfirmed means the founding waiver let them through;
            // the vote is theirs on trust until the deadline.
            membership.unconfirmed_franchise_until =
                Some(domain::confirmation_deadline(now, self.founding_grace_days));
        }
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

    /// Read-only: where a member stands on a vote held on trust — how long they have
    /// left to confirm, or whether they have already lost the vote by not doing so.
    /// A member with a confirmed address has nothing outstanding by definition.
    pub async fn trusted_franchise(&self, handle: &str, server_slug: &str) -> TrustedFranchise {
        let Some(user) = self.users.find_by_handle(handle.trim()).await.ok().flatten() else {
            return TrustedFranchise::default();
        };
        if user.is_email_verified() {
            return TrustedFranchise::default();
        }
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return TrustedFranchise::default();
        };
        let Some(m) = self.memberships.get(user.id, server.id).await.ok().flatten() else {
            return TrustedFranchise::default();
        };
        let now = self.clock.now();
        TrustedFranchise {
            days_left: m.days_to_confirm(now),
            lapsed: m.trusted_franchise_lapsed(now),
        }
    }

    /// A server's current [`Phase`], derived from its citizen count. Needed by every
    /// eligibility check because the founding cohort (Seed) is excused the wait.
    async fn phase_of(&self, server: domain::ServerId) -> Phase {
        Phase::from_citizen_count(
            self.memberships.citizen_count(server).await.unwrap_or_default(),
            self.phase_thresholds,
        )
    }

    /// Reconcile votes held on trust: clear the deadline for anyone who has since
    /// confirmed, and demote whoever let theirs run out.
    ///
    /// The demotion is bookkeeping, not enforcement —
    /// [`Membership::is_franchised`] already stopped counting a lapsed vote the
    /// instant it expired, in every path at once. What this adds is *visibility*:
    /// the member's tier goes back to "member", so the UI can say plainly that the
    /// vote is gone and how to get it back, rather than showing a citizen whose
    /// ballots quietly don't count. Confirming later re-admits them on the ordinary
    /// path — by then they have served the dwell the waiver excused, so
    /// [`auto_enfranchise`](Self::auto_enfranchise) simply seats them again.
    ///
    /// Idempotent and cheap on a settled server, so hot read paths can sweep freely.
    /// Returns how many memberships changed.
    pub async fn reconcile_trusted_franchise(&self, server_slug: &str) -> u64 {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return 0;
        };
        let now = self.clock.now();
        let mut changed = 0;
        for mut m in self.memberships.list_for_server(server.id).await.unwrap_or_default() {
            if m.unconfirmed_franchise_until.is_none() {
                continue;
            }
            let Some(user) = self.users.get_user(m.user_id).await.ok().flatten() else {
                continue;
            };
            if user.is_email_verified() {
                // Confirmed in time: the deadline has done its job and goes away.
                m.unconfirmed_franchise_until = None;
            } else if m.trusted_franchise_lapsed(now) && m.is_citizen() {
                // Out of time. The stale deadline stays on the record — it is what
                // stops the founding waiver handing out a second grace.
                m.tier = Tier::Member;
                m.enfranchised_at = None;
            } else {
                continue;
            }
            if self.memberships.upsert(m).await.is_ok() {
                changed += 1;
            }
        }
        changed
    }

    /// Automatically admit every member who now meets the franchise criteria —
    /// earliest joiner first — up to the rate cap. In production citizenship is not
    /// something a member asks for; it is conferred the moment the conditions hold.
    /// Idempotent and safe to call on any read (nothing happens once everyone
    /// eligible is admitted or the cap is full), so hot paths can sweep cheaply.
    /// Returns how many members were newly enfranchised.
    pub async fn auto_enfranchise(&self, server_slug: &str) -> u64 {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return 0;
        };
        let now = self.clock.now();
        let window_start = Timestamp(now.0 - RATE_CAP_WINDOW_DAYS * Timestamp::SECONDS_PER_DAY);
        // Read once, before anyone is seated: were the phase re-read per member, the
        // fifth admission would find the server already chartered and be judged by
        // criteria the four before it were excused.
        let phase = self.phase_of(server.id).await;

        // Collect the eligible non-citizens (a sanctioned member is barred), each
        // paired with whether they arrive with a confirmed address.
        let mut eligible: Vec<(Membership, bool)> = Vec::new();
        for m in self.memberships.list_for_server(server.id).await.unwrap_or_default() {
            if m.is_citizen() || m.is_sanctioned {
                continue;
            }
            let Some(user) = self.users.get_user(m.user_id).await.ok().flatten() else {
                continue;
            };
            if evaluate_eligibility(
                &user,
                &m,
                &server.criteria,
                phase,
                self.franchise_email_rule(),
                now,
            )
            .is_eligible()
            {
                // Remember whether this admission rests on trust: the seating step
                // below needs it, and by then the user record is out of reach.
                eligible.push((m, user.is_email_verified()));
            }
        }
        // Fair order: the earliest joiners qualified first, so they get the slots
        // first when the rate cap can't admit everyone at once.
        eligible.sort_by_key(|(m, _)| m.joined_at.0);

        let mut admitted = 0;
        for (mut m, confirmed) in eligible {
            m.tier = Tier::Citizen;
            m.enfranchised_at = Some(now);
            // A founding member seated without a confirmed address votes on trust,
            // and from here the clock is running (see `confirmation_deadline`).
            if !confirmed {
                m.unconfirmed_franchise_until =
                    Some(domain::confirmation_deadline(now, self.founding_grace_days));
            }
            match self
                .memberships
                .admit_within_cap(m, window_start, &enfranchisement_slots)
                .await
            {
                Ok(CapAdmission::Admitted) => admitted += 1,
                // Cap full for this window — everyone still queued waits their turn.
                Ok(CapAdmission::RateCapped { .. }) => break,
                Err(_) => continue,
            }
        }
        admitted
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
        Ok(evaluate_eligibility(
            &user,
            &membership,
            &server.criteria,
            self.phase_of(server.id).await,
            self.franchise_email_rule(),
            self.clock.now(),
        ))
    }

    /// Read-only: every server plus its phase and citizen count, for a directory.
    pub async fn list_servers(&self) -> Vec<(Server, Phase, u64)> {
        let mut out = Vec::new();
        for g in self.servers.list_all().await.unwrap_or_default() {
            let citizens = self.memberships.citizen_count(g.id).await.unwrap_or_default();
            let phase = Phase::from_citizen_count(citizens, self.phase_thresholds);
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
        Some((server.clone(), Phase::from_citizen_count(citizens, self.phase_thresholds), citizens))
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
