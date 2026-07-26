//! The platform-wide user account entity.

use serde::{Deserialize, Serialize};

use crate::{DmPolicy, Tags, Timestamp, UserId};

/// A platform-wide account. A user joins many servers; their standing *within* a
/// server lives in [`crate::Membership`], never here.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub handle: String,
    pub created_at: Timestamp,
    /// Whether this account is permanently barred from the franchise. Set on
    /// dev/content "puppet" accounts: they exist only to seed content and must
    /// NEVER become citizens. The domain enforces it —
    /// [`evaluate_eligibility`](crate::evaluate_eligibility) treats a barred
    /// account as never eligible — so the bar holds no matter which path is
    /// tried. `#[serde(default)]` keeps pre-flag datasets loadable.
    #[serde(default)]
    pub is_franchise_barred: bool,
    /// Who may open a direct message with this account. Defaults to
    /// [`DmPolicy::Everyone`] — DMs are on by default. `#[serde(default)]` keeps
    /// pre-flag datasets loadable.
    #[serde(default)]
    pub dm_policy: DmPolicy,
    /// The PHC-format Argon2 hash of the account's password, or empty if no
    /// password is set (a legacy/seed account that cannot log in until one is
    /// set). The domain never hashes or verifies — that is an `app` concern — it
    /// only carries the opaque string. `#[serde(default)]` keeps pre-auth datasets
    /// loadable.
    #[serde(default)]
    pub password_hash: String,
    /// Free-form self-description tags for this account. Empty by default;
    /// `#[serde(default)]` keeps pre-tags datasets loadable.
    #[serde(default)]
    pub tags: Tags,
    /// The account's email address **encrypted at rest** — an opaque ChaCha20-
    /// Poly1305 sealed blob produced by the `app` layer under the node's dedicated
    /// email key, or empty for seed/CLI accounts that never had one. Like
    /// [`password_hash`](Self::password_hash), the domain never decrypts or
    /// interprets it; a plaintext email is *never* stored here. Format validation
    /// happens on the plaintext in the `app` layer ([`crate::validate_email`])
    /// before sealing. `#[serde(default)]` keeps pre-email datasets loadable.
    #[serde(default)]
    pub email_enc: String,
    /// Whether the account's email has been confirmed via a verification link. New
    /// web signups start `false` and flip to `true` when the emailed token is spent;
    /// seed/CLI and `off`-mode accounts are created already `true`. `#[serde(default)]`
    /// keeps pre-email datasets loadable.
    #[serde(default)]
    pub email_verified: bool,
}

impl User {
    pub fn new(id: UserId, handle: impl Into<String>, created_at: Timestamp) -> Self {
        Self {
            id,
            handle: handle.into(),
            created_at,
            is_franchise_barred: false,
            dm_policy: DmPolicy::Everyone,
            password_hash: String::new(),
            tags: Tags::default(),
            email_enc: String::new(),
            email_verified: false,
        }
    }

    /// Whether this account has a password set (and so can authenticate).
    pub fn has_password(&self) -> bool {
        !self.password_hash.is_empty()
    }

    /// Whether this account has a (sealed) email on file.
    pub fn has_email(&self) -> bool {
        !self.email_enc.is_empty()
    }

    /// Whether this account's email has been verified (or verification is not
    /// required for it, e.g. seed/CLI accounts created already-verified).
    pub fn is_email_verified(&self) -> bool {
        self.email_verified
    }

    /// Whether this user only accepts DMs from accepted friends.
    pub fn is_friends_only_dm(&self) -> bool {
        matches!(self.dm_policy, DmPolicy::FriendsOnly)
    }

    /// Mark this account as permanently franchise-barred — a content-only "puppet"
    /// that can never become a citizen. Builder-style, so a barred account reads
    /// as `User::new(..).barred()`.
    pub fn barred(mut self) -> Self {
        self.is_franchise_barred = true;
        self
    }

    /// Whole days since the account was created.
    pub fn account_age_days(&self, now: Timestamp) -> i64 {
        now.days_since(self.created_at)
    }
}
