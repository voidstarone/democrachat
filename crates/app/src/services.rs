//! The use-cases — the single entry point a driving adapter (CLI, web) calls.
//!
//! Every method is a thin orchestration over the pure `domain` rules and the
//! `ports`: no business rule lives here that isn't ultimately a domain function.

use std::sync::Arc;

use domain::{
    enfranchisement_slots, evaluate_eligibility, slugify, Eligibility, Invite, Server, Membership,
    Phase, Tier, Timestamp, User,
};

use crate::outcome::EnfranchiseOutcome;
use crate::{
    BlockStore, ChannelKeyStore, ChannelStore, Clock, DmStore, EmailVerificationMode,
    EnfranchiseError, EmojiStore, EmojiVoteStore, FoundError, FriendStore, InviteError, InviteStore,
    JoinError, KeyDirectoryStore, MembershipStore, MediaStore, MessageStore, ProposalStore,
    ReactionStore, RegisterError, RoleStore, RuleStore, ServerStore, UserStore, VaultKey,
    VerificationTokenStore, VoteStore,
};

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

/// The trailing window the enfranchisement rate cap measures admissions over.
const RATE_CAP_WINDOW_DAYS: i64 = 30;

/// The driven ports the use-cases persist through. Bundled into one struct so
/// [`Services::new`] takes a single argument instead of a dozen — the composition
/// root fills it from whichever store(s) it chose.
#[derive(Clone)]
pub struct Stores {
    pub users: Arc<dyn UserStore>,
    pub servers: Arc<dyn ServerStore>,
    pub memberships: Arc<dyn MembershipStore>,
    pub channels: Arc<dyn ChannelStore>,
    pub messages: Arc<dyn MessageStore>,
    pub reactions: Arc<dyn ReactionStore>,
    pub proposals: Arc<dyn ProposalStore>,
    pub votes: Arc<dyn VoteStore>,
    pub emojis: Arc<dyn EmojiStore>,
    pub emoji_votes: Arc<dyn EmojiVoteStore>,
    pub rules: Arc<dyn RuleStore>,
    pub dms: Arc<dyn DmStore>,
    pub blocks: Arc<dyn BlockStore>,
    pub friends: Arc<dyn FriendStore>,
    pub roles: Arc<dyn RoleStore>,
    pub keys: Arc<dyn KeyDirectoryStore>,
    pub channel_keys: Arc<dyn ChannelKeyStore>,
    pub invites: Arc<dyn InviteStore>,
    pub media: Arc<dyn MediaStore>,
    pub verification_tokens: Arc<dyn VerificationTokenStore>,
}

#[derive(Clone)]
pub struct Services {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) users: Arc<dyn UserStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) channels: Arc<dyn ChannelStore>,
    pub(crate) messages: Arc<dyn MessageStore>,
    pub(crate) reactions: Arc<dyn ReactionStore>,
    pub(crate) proposals: Arc<dyn ProposalStore>,
    pub(crate) votes: Arc<dyn VoteStore>,
    pub(crate) emojis: Arc<dyn EmojiStore>,
    pub(crate) emoji_votes: Arc<dyn EmojiVoteStore>,
    pub(crate) rules: Arc<dyn RuleStore>,
    pub(crate) dms: Arc<dyn DmStore>,
    pub(crate) blocks: Arc<dyn BlockStore>,
    pub(crate) friends: Arc<dyn FriendStore>,
    pub(crate) roles: Arc<dyn RoleStore>,
    pub(crate) keys: Arc<dyn KeyDirectoryStore>,
    pub(crate) channel_keys: Arc<dyn ChannelKeyStore>,
    pub(crate) invites: Arc<dyn InviteStore>,
    pub(crate) media: Arc<dyn MediaStore>,
    pub(crate) verification_tokens: Arc<dyn VerificationTokenStore>,
    /// Verification policy (default [`EmailVerificationMode::Off`]); set by the
    /// composition root via [`with_email_policy`](Self::with_email_policy).
    pub(crate) email_verification: EmailVerificationMode,
    /// The dedicated key emails are sealed under, or `None` when no address is
    /// collected/stored (off mode / CLI / tests).
    pub(crate) email_key: Option<VaultKey>,
}

impl Services {
    pub fn new(clock: Arc<dyn Clock>, stores: Stores) -> Self {
        Self {
            clock,
            users: stores.users,
            servers: stores.servers,
            memberships: stores.memberships,
            channels: stores.channels,
            messages: stores.messages,
            reactions: stores.reactions,
            proposals: stores.proposals,
            votes: stores.votes,
            emojis: stores.emojis,
            emoji_votes: stores.emoji_votes,
            rules: stores.rules,
            dms: stores.dms,
            blocks: stores.blocks,
            friends: stores.friends,
            roles: stores.roles,
            keys: stores.keys,
            channel_keys: stores.channel_keys,
            invites: stores.invites,
            media: stores.media,
            verification_tokens: stores.verification_tokens,
            email_verification: EmailVerificationMode::Off,
            email_key: None,
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

    /// The deployment's email-verification policy, for the web layer's login gate.
    pub fn email_verification(&self) -> EmailVerificationMode {
        self.email_verification
    }

    /// Register a new platform account **without** a password. For seeding, the
    /// CLI, and tests only — such an account cannot authenticate to the web app
    /// ([`authenticate`](Self::authenticate) rejects a passwordless account) until
    /// a password is set. Web sign-up uses
    /// [`register_with_password`](Self::register_with_password).
    pub fn register_account(&self, handle: &str) -> Result<User, RegisterError> {
        let handle = handle.trim();
        if handle.is_empty() {
            return Err(RegisterError::EmptyHandle);
        }
        if self.users.find_by_handle(handle).is_some() {
            return Err(RegisterError::HandleTaken(handle.to_string()));
        }
        let mut user = User::new(self.users.next_user_id(), handle, self.clock.now());
        // Seed/CLI accounts carry no email and are exempt from the verification
        // gate, so they remain usable even in hard mode.
        user.email_verified = true;
        self.users.insert_user(user.clone());
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
    pub fn register_with_password(
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
        if self.users.find_by_handle(handle).is_some() {
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
        let mut user = User::new(self.users.next_user_id(), handle, self.clock.now());
        user.password_hash = hash;
        user.email_enc = email_enc;

        let verification_token =
            if self.email_verification.requires_verification() && self.email_key.is_some() {
                user.email_verified = false;
                let raw = crate::email::new_verification_token();
                let expires_at = self.clock.now().0 + VERIFICATION_TTL_SECS;
                self.verification_tokens
                    .add(crate::email::hash_token(&raw), user.id, expires_at);
                Some(raw)
            } else {
                user.email_verified = true;
                None
            };

        self.users.insert_user(user.clone());
        Ok(Registration { user, verification_token })
    }

    /// Consume a verification token, marking its account's email verified. Returns
    /// the updated account, or `None` if the token is unknown, already used, or
    /// expired. Single-use.
    pub fn verify_email(&self, raw_token: &str) -> Option<User> {
        let now = self.clock.now().0;
        let user_id = self
            .verification_tokens
            .take(&crate::email::hash_token(raw_token), now)?;
        let mut user = self.users.get_user(user_id)?;
        user.email_verified = true;
        self.users.update_user(user.clone());
        Some(user)
    }

    /// Re-issue a verification token for an as-yet-unverified account, returning the
    /// decrypted address and raw token for the web layer to email. `None` for every
    /// "nothing to do" case (verification off / no key, unknown handle, already
    /// verified, or no email on file) — the web layer maps them all to one opaque
    /// response so this is not an account-existence oracle.
    pub fn issue_resend(&self, handle: &str) -> Option<ResendTarget> {
        let key = self.email_key.as_ref()?;
        if !self.email_verification.requires_verification() {
            return None;
        }
        let user = self.users.find_by_handle(handle.trim())?;
        if user.email_verified || !user.has_email() {
            return None;
        }
        let email = crate::email::open_email(key, &user.email_enc).ok()?;
        let raw = crate::email::new_verification_token();
        let expires_at = self.clock.now().0 + VERIFICATION_TTL_SECS;
        self.verification_tokens
            .add(crate::email::hash_token(&raw), user.id, expires_at);
        Some(ResendTarget { email, token: raw })
    }

    /// Set (or replace) an account's password. Used to give seed/CLI accounts a
    /// real credential so the demo can log in as them.
    pub fn set_password(&self, handle: &str, password: &str) -> Result<(), RegisterError> {
        domain::validate_password(password)
            .map_err(|e| RegisterError::WeakPassword(e.to_string()))?;
        let mut user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or(RegisterError::EmptyHandle)?;
        user.password_hash = crate::hash_password(password).map_err(|_| RegisterError::HashFailed)?;
        self.users.update_user(user);
        Ok(())
    }

    /// Authenticate a handle + password, returning the account on success and
    /// `None` on any failure (unknown handle, passwordless account, or wrong
    /// password). Every failure path spends one Argon2 verify's worth of time
    /// ([`spend_verify_time`](crate::spend_verify_time)) so an attacker cannot
    /// tell "no such account" from "wrong password" by timing.
    pub fn authenticate(&self, handle: &str, password: &str) -> Option<User> {
        match self.users.find_by_handle(handle.trim()) {
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
    pub fn found_server(
        &self,
        founder_handle: &str,
        name: &str,
    ) -> Result<Server, FoundError> {
        self.found_server_with_visibility(founder_handle, name, false)
    }

    /// Found a server, choosing its visibility. A **private** server is hidden from
    /// the public browse directory and joinable only with an invite code; a public
    /// one is listed and freely joinable. Either way the founder becomes citizen #1
    /// and the invite policy starts [`InvitePolicy::Open`](domain::InvitePolicy) so
    /// the community can grow toward its first ten voters.
    pub fn found_server_with_visibility(
        &self,
        founder_handle: &str,
        name: &str,
        is_private: bool,
    ) -> Result<Server, FoundError> {
        let founder = self
            .users
            .find_by_handle(founder_handle.trim())
            .ok_or_else(|| FoundError::NoSuchUser(founder_handle.to_string()))?;
        if founder.is_franchise_barred {
            return Err(FoundError::FounderBarred);
        }
        let slug = slugify(name);
        if slug.is_empty() {
            return Err(FoundError::EmptyName);
        }
        if self.servers.find_by_slug(&slug).is_some() {
            return Err(FoundError::SlugTaken(slug));
        }

        let now = self.clock.now();
        let mut server =
            Server::new(self.servers.next_server_id(), slug, name.trim(), founder.id, now);
        server.is_private = is_private;
        self.servers.insert_server(server.clone());

        // Founder joins as citizen #1.
        let mut m = Membership::joined(founder.id, server.id, now);
        m.tier = Tier::Citizen;
        m.enfranchised_at = Some(now);
        self.memberships.upsert(m);

        Ok(server)
    }

    /// Mint an invite code for a server, returning the **raw** code to share (only
    /// its digest is stored). Gated on: the actor is a member, and the server's
    /// [`InvitePolicy`](domain::InvitePolicy) is `Open`. An invite grants membership
    /// only — never the franchise.
    pub fn create_invite(&self, handle: &str, server_slug: &str) -> Result<String, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        if !server.allows_member_invites() {
            return Err(InviteError::Closed(server_slug.to_string()));
        }
        let code = crate::invite::new_invite_code::new_invite_code();
        let hash = crate::invite::hash_code::hash_code(&code);
        self.invites
            .add(Invite::new(hash, server.id, user.id, self.clock.now()));
        Ok(code)
    }

    /// Redeem an invite code: join its server as an ordinary member. Fails if the
    /// code is unknown/revoked, the server has since closed invites, or the redeemer
    /// already belongs. Never grants citizenship.
    pub fn accept_invite(&self, handle: &str, code: &str) -> Result<Membership, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let hash = crate::invite::hash_code::hash_code(code);
        let invite = self
            .invites
            .by_hash(&hash)
            .filter(Invite::is_live)
            .ok_or(InviteError::InvalidCode)?;
        let server = self
            .servers
            .get_server(invite.server_id)
            .ok_or(InviteError::InvalidCode)?;
        if !server.allows_member_invites() {
            return Err(InviteError::Closed(server.slug.clone()));
        }
        if self.memberships.get(user.id, server.id).is_some() {
            return Err(InviteError::AlreadyMember(server.slug));
        }
        let m = Membership::joined(user.id, server.id, self.clock.now());
        self.memberships.upsert(m.clone());
        Ok(m)
    }

    /// The live invites for a server, for a member to view/share. Members only.
    pub fn list_invites(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<Vec<Invite>, InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        Ok(self
            .invites
            .list_for_server(server.id)
            .into_iter()
            .filter(Invite::is_live)
            .collect())
    }

    /// Revoke one of a server's invites by its code digest. Members only.
    pub fn revoke_invite(
        &self,
        handle: &str,
        server_slug: &str,
        code_hash: &str,
    ) -> Result<(), InviteError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| InviteError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| InviteError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).is_none() {
            return Err(InviteError::NotMember(server_slug.to_string()));
        }
        // Only revoke a hash that actually belongs to this server.
        if self
            .invites
            .by_hash(code_hash)
            .is_some_and(|i| i.server_id == server.id)
        {
            self.invites.revoke(code_hash);
        }
        Ok(())
    }

    /// Join a server as an ordinary member (tier `Member`). Joining accrues dwell
    /// time toward the franchise but confers **no** vote.
    pub fn join_server(&self, handle: &str, server_slug: &str) -> Result<Membership, JoinError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| JoinError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| JoinError::NoSuchServer(server_slug.to_string()))?;
        if self.memberships.get(user.id, server.id).is_some() {
            return Err(JoinError::AlreadyMember(server_slug.to_string()));
        }
        let m = Membership::joined(user.id, server.id, self.clock.now());
        self.memberships.upsert(m.clone());
        Ok(m)
    }

    /// Attempt to enfranchise a member. This is the **only** path by which a
    /// non-founder becomes a citizen, and it runs entirely on the domain rules:
    /// Layer 1 (are the criteria met?) then Layer 2 (is a rate-cap slot open?).
    /// There is no override.
    pub fn try_enfranchise(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<EnfranchiseOutcome, EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let mut membership = self
            .memberships
            .get(user.id, server.id)
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

        // Layer 2 — enfranchisement rate cap.
        let citizens = self.memberships.citizen_count(server.id);
        let window_start = Timestamp(now.0 - RATE_CAP_WINDOW_DAYS * Timestamp::SECONDS_PER_DAY);
        let admitted = self.memberships.admitted_since(server.id, window_start);
        if enfranchisement_slots(citizens, admitted) == 0 {
            return Ok(EnfranchiseOutcome::RateCapped {
                admitted_this_window: admitted,
            });
        }

        // Admit.
        membership.tier = Tier::Citizen;
        membership.enfranchised_at = Some(now);
        self.memberships.upsert(membership);
        Ok(EnfranchiseOutcome::Admitted)
    }

    /// Read-only: how a member currently stands against the franchise criteria.
    /// Powers a "why can't I vote yet?" view without any side effect.
    pub fn eligibility(
        &self,
        handle: &str,
        server_slug: &str,
    ) -> Result<Eligibility, EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let membership = self
            .memberships
            .get(user.id, server.id)
            .ok_or_else(|| EnfranchiseError::NotAMember(handle.to_string()))?;
        Ok(evaluate_eligibility(&user, &membership, &server.criteria, self.clock.now()))
    }

    /// Read-only: every server plus its phase and citizen count, for a directory.
    pub fn list_servers(&self) -> Vec<(Server, Phase, u64)> {
        self.servers
            .list_all()
            .into_iter()
            .map(|g| {
                let citizens = self.memberships.citizen_count(g.id);
                let phase = Phase::from_citizen_count(citizens);
                (g, phase, citizens)
            })
            .collect()
    }

    /// The servers this user belongs to — powers their sidebar. A user only ever
    /// sees servers they've joined (public or private); discovery of *new* servers
    /// is [`list_public_servers`](Self::list_public_servers).
    pub fn my_servers(&self, handle: &str) -> Vec<(Server, Phase, u64)> {
        let Some(user) = self.users.find_by_handle(handle.trim()) else {
            return Vec::new();
        };
        self.list_servers()
            .into_iter()
            .filter(|(g, _, _)| self.memberships.get(user.id, g.id).is_some())
            .collect()
    }

    /// The slug of a server by id, if it exists (for turning a redeemed invite's
    /// server id back into a URL the client can open).
    pub fn server_slug(&self, id: domain::ServerId) -> Option<String> {
        self.servers.get_server(id).map(|g| g.slug)
    }

    /// The public browse directory: every **public** server. Private servers are
    /// omitted — they are reachable only by invite code.
    pub fn list_public_servers(&self) -> Vec<(Server, Phase, u64)> {
        self.list_servers()
            .into_iter()
            .filter(|(g, _, _)| !g.is_private)
            .collect()
    }

    /// Read-only: look up an account by handle.
    pub fn find_user(&self, handle: &str) -> Option<User> {
        self.users.find_by_handle(handle.trim())
    }

    /// The current instant according to the injected clock.
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// Read-only: a member's tier within a server, if they belong.
    pub fn member_tier(&self, handle: &str, server_slug: &str) -> Option<Tier> {
        let user = self.users.find_by_handle(handle.trim())?;
        let server = self.servers.find_by_slug(server_slug.trim())?;
        self.memberships.get(user.id, server.id).map(|m| m.tier)
    }

    /// Read-only: a member's endorsement-weighted contribution in a server.
    pub fn member_contribution(&self, handle: &str, server_slug: &str) -> Option<i64> {
        let user = self.users.find_by_handle(handle.trim())?;
        let server = self.servers.find_by_slug(server_slug.trim())?;
        self.memberships.get(user.id, server.id).map(|m| m.contribution)
    }

    /// Read-only snapshot of a server: the server, its current phase, and its
    /// citizen count.
    pub fn server_snapshot(&self, server_slug: &str) -> Option<(Server, Phase, u64)> {
        let server = self.servers.find_by_slug(server_slug.trim())?;
        let citizens = self.memberships.citizen_count(server.id);
        Some((server.clone(), Phase::from_citizen_count(citizens), citizens))
    }

    /// Dev/testing helper: set a member's endorsement-weighted contribution
    /// directly. In the real system this score is produced by citizens reacting
    /// positively to a member's messages; here it lets the CLI demonstrate the
    /// franchise criteria without a full reaction pipeline. It changes *only* the
    /// contribution input to Layer 1 — it never enfranchises anyone.
    pub fn set_contribution(
        &self,
        handle: &str,
        server_slug: &str,
        contribution: i64,
    ) -> Result<(), EnfranchiseError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| EnfranchiseError::NoSuchServer(server_slug.to_string()))?;
        let mut membership = self
            .memberships
            .get(user.id, server.id)
            .ok_or_else(|| EnfranchiseError::NotAMember(handle.to_string()))?;
        membership.contribution = contribution;
        self.memberships.upsert(membership);
        Ok(())
    }
}
