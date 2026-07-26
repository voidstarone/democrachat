//! Request/response shapes for the JSON API. Grouped here (not one-per-file)
//! because they are trivial glue with no behaviour — they exist only to shuttle
//! primitives between the browser and the use-cases.

use serde::{Deserialize, Serialize};

/// Login/registration credentials. Identity everywhere else comes from the
/// session cookie — this is the *only* place a handle is accepted from the body,
/// and only alongside a password that must verify.
#[derive(Deserialize)]
pub struct CredentialsReq {
    pub handle: String,
    pub password: String,
    /// The email address, required for registration and ignored for login.
    /// `#[serde(default)]` so a login body (`{handle, password}`) still
    /// deserializes without it.
    #[serde(default)]
    pub email: String,
}

/// Query for the email-verification link (`GET /verify?token=…`).
#[derive(Deserialize)]
pub struct VerifyQuery {
    pub token: String,
}

/// Request a fresh verification email for an unverified account.
#[derive(Deserialize)]
pub struct ResendReq {
    pub handle: String,
}

#[derive(Deserialize)]
pub struct FoundReq {
    pub name: String,
    /// Found the server private (hidden from the directory, invite-only). Defaults
    /// to public.
    #[serde(default)]
    pub is_private: bool,
}

#[derive(Deserialize)]
pub struct CreateChannelReq {
    pub name: String,
    #[serde(default)]
    pub topic: String,
}

#[derive(Deserialize)]
pub struct PostReq {
    pub body: String,
    #[serde(default)]
    pub parent: Option<u64>,
    /// Media to attach (top-level messages only; ignored on replies). Each was
    /// pre-uploaded via `POST /api/media`.
    #[serde(default)]
    pub attachments: Vec<AttachmentReq>,
}

#[derive(Deserialize)]
pub struct ReactReq {
    pub emoji: String,
}

#[derive(Deserialize)]
pub struct AdvanceReq {
    pub days: i64,
}

#[derive(Serialize)]
pub struct ServerSummary {
    pub slug: String,
    pub name: String,
    pub phase: String,
    pub citizens: u64,
    /// Whether the server is private (hidden from the directory, invite-only).
    #[serde(default)]
    pub is_private: bool,
    /// Whether any member may currently mint invite codes (policy is `Open`).
    #[serde(default)]
    pub are_invites_open: bool,
}

/// One live invite in a server's list. Carries only the code *digest* — the raw
/// code is shown once, at mint time, and never again (it is unrecoverable here).
#[derive(Serialize)]
pub struct InviteDto {
    pub code_hash: String,
    pub created_at: i64,
}

/// Redeem an invite code to join its server.
#[derive(Deserialize)]
pub struct AcceptInviteReq {
    pub code: String,
}

#[derive(Serialize)]
pub struct ChannelDto {
    pub name: String,
    pub topic: String,
    /// Whether message bodies in this channel are end-to-end encrypted.
    pub is_encrypted: bool,
    /// "open" or "ephemeral" — only meaningful when encrypted.
    pub history_mode: String,
}

/// Turn on encryption for a channel. `history_mode` is "open" or "ephemeral".
#[derive(Deserialize)]
pub struct EnableEncryptionReq {
    #[serde(default)]
    pub history_mode: String,
}

/// Publish a sealed channel-key grant for a member (the client sealed the key).
#[derive(Deserialize)]
pub struct GrantKeyReq {
    pub epoch: u32,
    pub member: String,
    pub sealed_key: String,
}

/// One of the caller's own channel-key grants.
#[derive(Serialize)]
pub struct ChannelGrantDto {
    pub epoch: u32,
    pub sealed_key: String,
}

/// Post an end-to-end-encrypted message: the client sealed `ciphertext` under the
/// channel key of `key_epoch`.
#[derive(Deserialize)]
pub struct PostSealedReq {
    pub ciphertext: String,
    pub key_epoch: u32,
    #[serde(default)]
    pub parent: Option<u64>,
}

#[derive(Serialize)]
pub struct ServerDetail {
    pub slug: String,
    pub name: String,
    pub phase: String,
    pub citizens: u64,
    pub founder: String,
    pub min_account_age_days: i64,
    pub min_membership_days: i64,
    pub min_contribution: i64,
    pub surface: Vec<String>,
    /// How this server weights a citizen's vote (`Equal`, `ByContribution`, …).
    pub vote_weighting: String,
    /// Which decisions the weighting applies to (`Both`, `JuriesOnly`, …).
    pub weighting_scope: String,
    /// How this server sizes a trial jury from its citizen count.
    pub jury_sizing: JurySizingDto,
    pub channels: Vec<ChannelDto>,
    /// Whether the server is private (invite-only, hidden from the directory).
    #[serde(default)]
    pub is_private: bool,
    /// Whether any member may currently mint invite codes (policy is `Open`).
    #[serde(default)]
    pub are_invites_open: bool,
}

/// A server's jury-sizing rule, flattened for the client. `mode` is the variant
/// (`Sqrt` | `Proportion` | `Fixed`); `post`/`comment` are its two parameters
/// (basis points for `Sqrt`/`Proportion`, absolute juror counts for `Fixed`).
#[derive(Serialize)]
pub struct JurySizingDto {
    pub mode: String,
    pub post: u32,
    pub comment: u32,
}

/// One resolved `@mention` inside a message body.
#[derive(Serialize)]
pub struct MentionDto {
    /// The token as written (without `@`), for highlighting in the body.
    pub token: String,
    /// "user", "standing_role", "role", or "unknown".
    pub kind: String,
}

/// A compact reference to the message a reply is answering, rendered as a quote
/// above the reply with a jump link.
#[derive(Serialize)]
pub struct ReplyRefDto {
    pub id: u64,
    pub author: String,
    /// A one-line excerpt of the original message body.
    pub excerpt: String,
}

#[derive(Serialize)]
pub struct MessageDto {
    pub id: u64,
    pub author: String,
    /// Plaintext body, or — for a sealed message (`key_epoch` set) — the ciphertext
    /// the client decrypts with its channel-key grant.
    pub body: String,
    /// The channel-key epoch the body is sealed under, or `null` if plaintext.
    pub key_epoch: Option<u32>,
    /// When the message was posted (epoch seconds), for client-side formatting.
    pub ts: i64,
    pub parent: Option<u64>,
    pub is_deleted: bool,
    pub edited: bool,
    /// (emoji, count) pairs.
    pub reactions: Vec<(String, u64)>,
    /// Resolved `@mentions` in the body (for highlighting).
    pub mentions: Vec<MentionDto>,
    /// Whether the requesting viewer is addressed by this message — either by an
    /// explicit `@mention` or by being the author of the message this replies to.
    pub mentions_me: bool,
    /// The message this one replies to, if any (a quote + jump link).
    pub reply_to: Option<ReplyRefDto>,
    /// Media attached to this message (empty for text-only).
    #[serde(default)]
    pub attachments: Vec<AttachmentDto>,
}

/// A media attachment as sent to the client: a URL to fetch the blob, its type,
/// and its spoiler flag.
#[derive(Serialize)]
pub struct AttachmentDto {
    /// Where to fetch the media: `/media/{key}`.
    pub url: String,
    pub content_type: String,
    /// "image", "video", or "audio" — which element to render.
    pub kind: String,
    pub caption: String,
    pub is_spoiler: bool,
}

/// One attachment reference in a post request. The bytes were already uploaded via
/// `POST /api/media` (which returned `key` + `content_type`); the client echoes
/// those back with its per-attachment caption/spoiler choices.
#[derive(Deserialize)]
pub struct AttachmentReq {
    pub key: String,
    pub content_type: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub is_spoiler: bool,
}

/// A custom role plus its current holders, for the governance panel.
#[derive(Serialize)]
pub struct RoleDto {
    pub id: u64,
    pub name: String,
    pub holders: Vec<String>,
}

/// The names a client can `@mention` on a server, for autocomplete.
#[derive(Serialize)]
pub struct MentionableDto {
    pub users: Vec<String>,
    pub roles: Vec<String>,
}

/// A proposal a client may open. Tagged by `kind`; only the demo-supported set.
#[derive(Deserialize)]
#[serde(tag = "kind")]
pub enum ProposeKindReq {
    AddRule { text: String },
    RemoveRule { rule: u64 },
    CreateChannel { name: String, #[serde(default)] topic: String },
    DeleteChannel { name: String },
    /// Ban a member, addressed by handle (resolved to an id server-side).
    Ban { handle: String },
    /// Create a custom mention role.
    CreateRole { name: String },
    /// Delete a custom role by name (resolved to an id server-side).
    DeleteRole { role: String },
    /// Add a member (by handle) to a role (by name).
    AssignRole { handle: String, role: String },
    /// Remove a member (by handle) from a role (by name).
    UnassignRole { handle: String, role: String },
    /// Enable or disable automatic rehoming of this server across the federation.
    SetRehomingPolicy { is_disabled: bool },
    /// Open or close who may mint invite codes (`is_open: false` seals admission).
    SetInvitePolicy { is_open: bool },
    /// Amend the franchise criteria — who may earn the vote here.
    AmendCriteria {
        min_account_age_days: i64,
        min_membership_days: i64,
        min_contribution: i64,
    },
    /// Change how a citizen's vote is weighted (variant name of `VoteWeighting`).
    SetVoteWeighting { scheme: String },
    /// Change which decisions the weighting applies to (variant name of `WeightingScope`).
    SetWeightingScope { scope: String },
    /// Change how a trial jury is sized. `mode` is the `JurySizing` variant;
    /// `post`/`comment` are its two parameters.
    SetJurySizing { mode: String, post: u32, comment: u32 },
    /// Replace the governance surface — the set of ballot kinds this server votes
    /// on. Each entry is a `BallotKind` variant name; unknown/always-on handled server-side.
    SetGovernanceSurface { kinds: Vec<String> },
}

#[derive(Deserialize)]
pub struct ProposeReq {
    #[serde(flatten)]
    pub kind: ProposeKindReq,
}

#[derive(Deserialize)]
pub struct VoteReq {
    pub is_aye: bool,
}

#[derive(Serialize)]
pub struct ProposalDto {
    pub id: u64,
    pub summary: String,
    pub proposer: String,
    /// "open", "passed", or "failed".
    pub status: String,
    pub aye: u64,
    pub nay: u64,
    /// Present only while open: seconds until the voting window closes.
    pub closes_in: Option<i64>,
    /// This viewer's vote: true=aye, false=nay, null=hasn't voted.
    pub my_vote: Option<bool>,
    /// Whether the passed effect has been applied yet (false during timelock).
    pub is_applied: bool,
}

/// The password-wrapped device secret — an opaque blob the server stores and
/// returns verbatim. Mirrors [`domain::WrappedKey`]; all fields are hex.
#[derive(Deserialize, Serialize)]
pub struct WrappedKeyDto {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

impl From<WrappedKeyDto> for domain::WrappedKey {
    fn from(w: WrappedKeyDto) -> Self {
        domain::WrappedKey { salt: w.salt, nonce: w.nonce, ciphertext: w.ciphertext }
    }
}

impl From<domain::WrappedKey> for WrappedKeyDto {
    fn from(w: domain::WrappedKey) -> Self {
        WrappedKeyDto { salt: w.salt, nonce: w.nonce, ciphertext: w.ciphertext }
    }
}

/// Publish this user's device keys: the public X25519 key plus the password-wrapped
/// secret. Sent by the client after it generates and wraps the identity locally.
#[derive(Deserialize)]
pub struct PublishKeysReq {
    pub public_key: String,
    pub wrapped_secret: WrappedKeyDto,
}

/// The caller's own directory entry, handed back so a new device can unwrap the
/// secret with the password.
#[derive(Serialize)]
pub struct MyKeysDto {
    pub public_key: String,
    pub wrapped_secret: WrappedKeyDto,
}

/// Another user's public key, for sealing a DM or message-key to them.
#[derive(Serialize)]
pub struct PublicKeyDto {
    pub handle: String,
    pub public_key: String,
}

/// Add a custom emoji. Provide **either** an uploaded `image` (base64 of a
/// ≤256×256 PNG/GIF, which becomes a `data:` URI) or an external `url`.
#[derive(Deserialize)]
pub struct AddEmojiReq {
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub image: Option<String>,
}

/// Cast (or change) an up/down vote on a custom emoji.
#[derive(Deserialize)]
pub struct VoteEmojiReq {
    pub is_up: bool,
}

/// One custom emoji in a server's ranked vote list.
#[derive(Serialize)]
pub struct EmojiDto {
    pub id: u64,
    pub name: String,
    pub url: String,
    /// Net score: citizen upvotes − downvotes.
    pub score: i64,
    /// "active", "considered", or "archived".
    pub standing: String,
    /// Whether this emoji is usable in new messages / the picker.
    pub is_active: bool,
    /// The viewing citizen's own vote: true=up, false=down, null=none.
    pub my_vote: Option<bool>,
}

/// Send an end-to-end-encrypted DM. The client seals the body against the
/// recipient's and its own device keys (from the key directory) and sends the two
/// opaque ciphertexts — the server never receives plaintext.
#[derive(Deserialize)]
pub struct SendDmReq {
    pub sealed_for_recipient: String,
    pub sealed_for_sender: String,
}

#[derive(Deserialize)]
pub struct DmPolicyReq {
    /// Whether DMs are restricted to friends only.
    pub is_friends_only: bool,
}

#[derive(Serialize)]
pub struct DmMessageDto {
    pub id: u64,
    pub sender: String,
    pub recipient: String,
    /// The ciphertext this viewer can open — sealed to their own device key. The
    /// client decrypts it locally; the server hands over a blob it cannot read.
    pub sealed_for_me: String,
    /// Whether this message was sent by the viewer.
    pub is_mine: bool,
}

/// A person in the viewer's DM sidebar, with the relationship state that drives
/// the block/friend controls.
#[derive(Serialize)]
pub struct DmPartnerDto {
    pub handle: String,
    pub is_friend: bool,
    pub is_blocked: bool,
    /// Whether the viewer can currently message this person.
    pub can_dm: bool,
}

/// The viewer's own social state — friends, pending requests, DM policy.
#[derive(Serialize)]
pub struct SocialMeDto {
    pub handle: String,
    pub is_friends_only: bool,
    pub partners: Vec<DmPartnerDto>,
    pub friends: Vec<String>,
    pub incoming_requests: Vec<String>,
    /// Users this user has blocked (permanent; display-only, no unblock).
    pub blocked: Vec<String>,
}

#[derive(Serialize)]
pub struct MeDto {
    /// "guest" (not a member), "member", or "citizen".
    pub tier: String,
    pub is_eligible: bool,
    /// Human-readable unmet requirements.
    pub unmet: Vec<String>,
    pub contribution: i64,
    /// The caller's personal per-server choice of whether members who join later
    /// may read the messages they have already posted (`true` = share; the default).
    /// `null` for a non-member (the setting only exists for members).
    #[serde(default)]
    pub shares_history: Option<bool>,
}

/// Toggle the caller's personal history-sharing preference for a server.
#[derive(Deserialize)]
pub struct HistorySharingReq {
    pub shares: bool,
}
