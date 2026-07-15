//! Chat use-cases: channels, messages, threaded replies, and reactions.
//!
//! The one governance touch-point here is **who may create a channel**: in Seed
//! the founder provisions the server; past Seed that becomes a ballot (M3). The
//! payoff of this milestone is that a **citizen's reaction endorses** a message's
//! author — the first real driver of the `contribution` score that Layer 1 of the
//! franchise consumes. An endorsement counts once per (message, citizen),
//! regardless of how many emojis they pile on, and is withdrawn if they clear
//! their reactions.

use std::sync::Arc;

use domain::{Channel, Message, Phase, Reaction};

use crate::{
    ChannelError, ChannelStore, Clock, ImageTranscoder, MediaStore, MembershipStore, MessageError,
    MessageStore, ReactionError, ReactionStore, ServerStore, UserStore,
};

/// Chat use-cases held on their own handle, reached via [`Services::chat`].
#[derive(Clone)]
pub struct ChatService {
    pub(crate) channels: Arc<dyn ChannelStore>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) image: Arc<dyn ImageTranscoder>,
    pub(crate) media: Arc<dyn MediaStore>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) messages: Arc<dyn MessageStore>,
    pub(crate) reactions: Arc<dyn ReactionStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

/// Per-file upload cap for a media attachment (25 MiB).
const MAX_MEDIA_BYTES: usize = 25 * 1024 * 1024;

/// Cap on how many attachments a single message may carry — a bound on both the
/// message size and the disk a post can claim.
const MAX_ATTACHMENTS: usize = 10;

/// Reject uploads whose leading bytes look like HTML/XML/SVG markup, regardless
/// of the declared MIME type. Served same-origin, such a blob is an XSS vector,
/// so a script-bearing SVG relabelled `image/png` must not slip past the
/// top-level type allowlist. (`nosniff` already stops the browser rendering a
/// blob as markup; this closes the vector at the source, as defence in depth.)
fn looks_like_markup(bytes: &[u8]) -> bool {
    let mut b = bytes;
    // Step over a UTF-8 or UTF-16 byte-order mark, then leading whitespace.
    for bom in [&[0xEF, 0xBB, 0xBF][..], &[0xFF, 0xFE][..], &[0xFE, 0xFF][..]] {
        if b.starts_with(bom) {
            b = &b[bom.len()..];
            break;
        }
    }
    let head: Vec<u8> = b
        .iter()
        .skip_while(|c| c.is_ascii_whitespace())
        .take(64)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    const OPENERS: [&[u8]; 7] = [
        b"<?xml", b"<svg", b"<!doctype", b"<html", b"<head", b"<body", b"<script",
    ];
    OPENERS.iter().any(|o| head.starts_with(o))
}

impl ChatService {
    /// Create a channel. Allowed only while the server is in **Seed** and only by
    /// its founder (provisioning) — a larger server governs its channels by ballot.
    pub async fn create_channel(
        &self,
        founder_handle: &str,
        server_slug: &str,
        name: &str,
        topic: &str,
    ) -> Result<Channel, ChannelError> {
        let user = self
            .users
            .find_by_handle(founder_handle.trim()).await?
            .ok_or_else(|| ChannelError::NoSuchUser(founder_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| ChannelError::NoSuchServer(server_slug.to_string()))?;

        let citizens = self.memberships.citizen_count(server.id).await?;
        let phase = Phase::from_citizen_count(citizens);
        if !(phase.founder_may_provision() && user.id == server.founder_id) {
            return Err(ChannelError::NotProvisionable);
        }

        let name = domain::normalize_channel_name(name);
        if name.is_empty() {
            return Err(ChannelError::EmptyName);
        }
        if self.channels.find_by_name(server.id, &name).await?.is_some() {
            return Err(ChannelError::NameTaken(name));
        }

        let channel = Channel::new(
            self.channels.next_channel_id().await?,
            server.id,
            name,
            topic.trim(),
            self.clock.now(),
        );
        self.channels.insert_channel(channel.clone()).await?;
        Ok(channel)
    }

    /// List a server's channels (in id order). `None` if no such server.
    pub async fn list_channels(&self, server_slug: &str) -> Option<Vec<Channel>> {
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        Some(self.channels.list_for_server(server.id).await.unwrap_or_default())
    }

    /// A server's channels as `viewer_handle` may see them — the restricted
    /// `#appeals` channel is hidden from anyone who is not a voter, police officer,
    /// or the muted appellant. `None` if no such server.
    pub async fn visible_channels(&self, viewer_handle: &str, server_slug: &str) -> Option<Vec<Channel>> {
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        let sees_appeals = match self.users.find_by_handle(viewer_handle.trim()).await.ok().flatten() {
            Some(u) => self
                .memberships
                .get(u.id, server.id).await.ok().flatten()
                .is_some_and(|m| crate::mute_service::membership_sees_appeals(&m)),
            None => false,
        };
        Some(
            self.channels
                .list_for_server(server.id).await.unwrap_or_default()
                .into_iter()
                .filter(|c| sees_appeals || !c.visibility.is_appeals())
                .collect(),
        )
    }

    /// Read-only: a user's handle by id (for rendering authorship).
    pub async fn user_handle(&self, id: domain::UserId) -> Option<String> {
        self.users.get_user(id).await.ok().flatten().map(|u| u.handle)
    }

    /// Read-only: a user's id by handle. The federation vote router needs the
    /// numeric id (handles are for humans; the wire carries ids).
    pub async fn user_id(&self, handle: &str) -> Option<u64> {
        self.users.find_by_handle(handle.trim()).await.ok().flatten().map(|u| u.id.0)
    }

    /// Read-only: a message's reactions, summarized per emoji in stable order.
    pub async fn message_reactions(&self, message_id: u64) -> Vec<(String, u64)> {
        let rs = self.reactions.list_for_message(domain::MessageId(message_id)).await.unwrap_or_default();
        domain::summarize_reactions(&rs)
    }

    /// Read-only: which (server slug, channel name) a message lives in — so a
    /// realtime event can be routed to the right channel view.
    pub async fn message_context(&self, message_id: u64) -> Option<(String, String)> {
        let m = self.messages.get_message(domain::MessageId(message_id)).await.ok().flatten()?;
        let server = self.servers.get_server(m.server_id).await.ok().flatten()?;
        let channel = self.channels.get_channel(m.channel_id).await.ok().flatten()?;
        Some((server.slug, channel.name))
    }

    /// Post a top-level message to a channel. Requires server membership.
    pub async fn post_message(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        body: &str,
    ) -> Result<Message, MessageError> {
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name).await?;
        self.insert_message(author_id, &channel, body, None, Vec::new()).await
    }

    /// Post a plaintext message with media attachments. The blobs must already be in
    /// the media store (via [`store_media`](Self::store_media)); this only records
    /// the references on the message. Media-only messages (empty body) are allowed.
    pub async fn post_message_with_attachments(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        body: &str,
        attachments: Vec<domain::Attachment>,
    ) -> Result<Message, MessageError> {
        if attachments.len() > MAX_ATTACHMENTS {
            return Err(MessageError::TooManyAttachments(MAX_ATTACHMENTS));
        }
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name).await?;
        self.insert_message(author_id, &channel, body, None, attachments).await
    }

    /// Validate and store an uploaded media blob, returning its storage key, the
    /// **actual stored content type**, and derived [`MediaKind`]. Rejects empty
    /// uploads, oversized files, and any MIME type that is not image/video/audio.
    /// The bytes go to the media store (a separate storage tier), keyed opaquely.
    ///
    /// The returned content type is what was *stored*, which for an image may
    /// differ from the declared one (an image is re-encoded — HEIC/HEIF → JPEG —
    /// so the caller records the true type on the message).
    pub fn store_media(
        &self,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(String, String, domain::MediaKind), crate::MediaError> {
        if bytes.is_empty() {
            return Err(crate::MediaError::Empty);
        }
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err(crate::MediaError::TooLarge);
        }
        // Ignore any `; charset=…` / parameters when classifying.
        let base = content_type.split(';').next().unwrap_or(content_type).trim().to_ascii_lowercase();
        // SVG is nominally an image but can carry scripts; served same-origin it is an
        // XSS vector, so it is not an accepted upload type.
        if base == "image/svg+xml" || looks_like_markup(bytes) {
            return Err(crate::MediaError::UnsupportedType(base));
        }
        let kind = domain::MediaKind::from_content_type(&base)
            .ok_or_else(|| crate::MediaError::UnsupportedType(base.clone()))?;
        // Images are re-encoded before storage: strips EXIF and any hostile
        // payload, and turns HEIC/HEIF (which browsers can't show) into JPEG. The
        // transcoder may change the content type (e.g. `image/heic` → `image/jpeg`),
        // so store — and report — whatever it returns. Video/audio are stored
        // verbatim.
        if kind == domain::MediaKind::Image {
            let (ct, data) = self.image.normalize(&base, bytes)?;
            let key = self.media.put(&ct, &data)?;
            Ok((key, ct, kind))
        } else {
            let key = self.media.put(&base, bytes)?;
            Ok((key, base, kind))
        }
    }

    /// Fetch a stored media blob and its content type for serving.
    pub fn media_blob(&self, key: &str) -> Option<(String, Vec<u8>)> {
        self.media.get(key)
    }

    /// Post an **end-to-end-encrypted** message: the client has already sealed the
    /// body under the channel key of `key_epoch`, so `ciphertext` is opaque to the
    /// server. Only valid in an encrypted channel; requires membership. An optional
    /// `parent` threads the reply (it must live in the same channel).
    pub async fn post_sealed_message(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        ciphertext: &str,
        key_epoch: u32,
        parent: Option<u64>,
    ) -> Result<Message, MessageError> {
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name).await?;
        if !channel.is_encrypted {
            return Err(MessageError::NotEncrypted);
        }
        let ciphertext = ciphertext.trim();
        if ciphertext.is_empty() {
            return Err(MessageError::EmptyBody);
        }
        let parent = match parent {
            Some(pid) => {
                let p = self
                    .messages
                    .get_message(domain::MessageId(pid)).await?
                    .ok_or(MessageError::NoSuchMessage(pid))?;
                if p.channel_id != channel.id {
                    return Err(MessageError::CrossChannelReply);
                }
                Some(p.id)
            }
            None => None,
        };
        let message = Message::new(
            self.messages.next_message_id().await?,
            channel.id,
            channel.server_id,
            author_id,
            ciphertext,
            parent,
            self.clock.now(),
        )
        .seal_under(key_epoch);
        self.messages.insert_message(message.clone()).await?;
        Ok(message)
    }

    /// Reply to an existing message, threading under it. Requires membership.
    pub async fn reply_message(
        &self,
        handle: &str,
        parent_id: u64,
        body: &str,
    ) -> Result<Message, MessageError> {
        let parent = self
            .messages
            .get_message(domain::MessageId(parent_id)).await?
            .ok_or(MessageError::NoSuchMessage(parent_id))?;
        let channel = self
            .channels
            .get_channel(parent.channel_id).await?
            .ok_or_else(|| MessageError::NoSuchChannel(parent.channel_id.to_string()))?;
        let server = self
            .servers
            .get_server(parent.server_id).await?
            .ok_or_else(|| MessageError::NoSuchServer(parent.server_id.to_string()))?;

        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        match self.memberships.get(user.id, server.id).await? {
            None => return Err(MessageError::NotAMember(handle.to_string())),
            Some(m) if m.is_sanctioned => return Err(MessageError::Sanctioned(handle.to_string())),
            Some(_) => {}
        }
        self.insert_message(user.id, &channel, body, Some(parent.id), Vec::new()).await
    }

    /// Edit a message's body. Author only.
    pub async fn edit_message(
        &self,
        handle: &str,
        message_id: u64,
        body: &str,
    ) -> Result<Message, MessageError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(MessageError::EmptyBody);
        }
        let mut message = self.authored_message(handle, message_id).await?;
        message.edit(body, self.clock.now());
        self.messages.update_message(message.clone()).await?;
        Ok(message)
    }

    /// Delete (tombstone) a message. Author only. Replies keep their place.
    pub async fn delete_message(&self, handle: &str, message_id: u64) -> Result<(), MessageError> {
        let mut message = self.authored_message(handle, message_id).await?;
        // Media lives and dies with its message: drop the blobs before tombstoning
        // (which clears the attachment list).
        for a in &message.attachments {
            self.media.delete(&a.key);
        }
        message.tombstone();
        self.messages.update_message(message).await?;
        Ok(())
    }

    /// The messages of a channel, flat and in post order, for the caller to build
    /// into a thread tree.
    pub async fn channel_messages(
        &self,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<Vec<Message>, MessageError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name).await?
            .ok_or_else(|| MessageError::NoSuchChannel(channel_name.to_string()))?;
        Ok(self.messages.list_for_channel(channel.id).await?)
    }

    /// A channel's messages as visible to `viewer`, honouring each author's
    /// personal history-sharing setting
    /// ([`Membership::shows_message_to`](domain::Membership::shows_message_to)):
    /// a member who has turned sharing off hides the messages they posted before
    /// `viewer` joined the server. The author and members already present when a
    /// message was posted always see it. An unknown or non-member viewer is treated
    /// as joining *now*, so they see only history authors have chosen to share.
    /// Applies uniformly to plaintext and sealed (E2EE) channels — filtering is by
    /// author and timestamp, which the server knows even for ciphertext.
    pub async fn channel_messages_for(
        &self,
        viewer_handle: &str,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<Vec<Message>, MessageError> {
        let all = self.channel_messages(server_slug, channel_name).await?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let viewer_id = self.users.find_by_handle(viewer_handle.trim()).await?.map(|u| u.id);
        // The viewer's own membership, fetched once.
        let viewer_membership = match viewer_id {
            Some(id) => self.memberships.get(id, server.id).await.ok().flatten(),
            None => None,
        };
        // Gate the restricted appeals channel: to anyone who is not a voter, police
        // officer, or the muted appellant, it reads as nonexistent.
        let cname = domain::normalize_channel_name(channel_name);
        if let Some(ch) = self.channels.find_by_name(server.id, &cname).await? {
            if ch.visibility.is_appeals() {
                let can = viewer_membership
                    .as_ref()
                    .is_some_and(crate::mute_service::membership_sees_appeals);
                if !can {
                    return Err(MessageError::NoSuchChannel(channel_name.to_string()));
                }
            }
        }
        let viewer_joined = viewer_membership
            .map(|m| m.joined_at)
            .unwrap_or_else(|| self.clock.now());
        // Pre-fetch each distinct author's membership (await can't live in a filter closure).
        let mut author_cache: std::collections::HashMap<domain::UserId, Option<domain::Membership>> =
            std::collections::HashMap::new();
        let mut out = Vec::new();
        for m in all {
            if viewer_id == Some(m.author) {
                out.push(m); // you always see your own messages
                continue;
            }
            if !author_cache.contains_key(&m.author) {
                let membership = self.memberships.get(m.author, server.id).await.ok().flatten();
                author_cache.insert(m.author, membership);
            }
            let visible = match &author_cache[&m.author] {
                Some(a) => a.shows_message_to(m.created_at, viewer_joined),
                None => true, // author left/unknown → nothing to hide
            };
            if visible {
                out.push(m);
            }
        }
        Ok(out)
    }

    /// This member's personal history-sharing preference for a server, if they are
    /// a member. `true` (share with newcomers) is the default.
    pub async fn history_sharing(&self, handle: &str, server_slug: &str) -> Option<bool> {
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        self.memberships
            .get(user.id, server.id).await.ok().flatten()
            .map(|m| m.shares_history_with_newcomers)
    }

    /// Set the caller's personal history-sharing preference for a server: whether
    /// members who join *after* their messages may read them. A personal, per-server
    /// control — it never affects members already present when a message was posted.
    pub async fn set_history_sharing(
        &self,
        handle: &str,
        server_slug: &str,
        shares: bool,
    ) -> Result<(), MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let mut m = self
            .memberships
            .get(user.id, server.id).await?
            .ok_or_else(|| MessageError::NotAMember(handle.to_string()))?;
        m.shares_history_with_newcomers = shares;
        self.memberships.upsert(m).await?;
        Ok(())
    }

    /// React to a message with an emoji. Requires membership. A **citizen's**
    /// reaction endorses the author, raising their contribution once per
    /// (message, citizen).
    pub async fn react(
        &self,
        handle: &str,
        message_id: u64,
        emoji: &str,
    ) -> Result<(), ReactionError> {
        let emoji = emoji.trim();
        if emoji.is_empty() {
            return Err(ReactionError::EmptyEmoji);
        }
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| ReactionError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id)).await?
            .ok_or(ReactionError::NoSuchMessage(message_id))?;
        let reactor = self
            .memberships
            .get(user.id, message.server_id).await?
            .ok_or_else(|| ReactionError::NotAMember(handle.to_string()))?;

        // Endorsement: a franchised citizen reacting (for the first time) to
        // someone else's message raises that author's contribution by one.
        let first_reaction = !self.reactions.user_has_any(message.id, user.id).await?;
        let endorses = first_reaction && reactor.is_franchised() && user.id != message.author;

        let added = self.reactions.add(Reaction::new(message.id, user.id, emoji)).await?;
        if added && endorses {
            self.adjust_contribution(message.author, message.server_id, 1).await;
        }
        Ok(())
    }

    /// Remove a reaction. If it was a citizen's sole reaction on the message, the
    /// endorsement is withdrawn.
    pub async fn unreact(
        &self,
        handle: &str,
        message_id: u64,
        emoji: &str,
    ) -> Result<(), ReactionError> {
        let emoji = emoji.trim();
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| ReactionError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id)).await?
            .ok_or(ReactionError::NoSuchMessage(message_id))?;

        let removed = self.reactions.remove(message.id, user.id, emoji).await?;
        if removed {
            let still_reacting = self.reactions.user_has_any(message.id, user.id).await?;
            if let Some(reactor) = self.memberships.get(user.id, message.server_id).await? {
                if !still_reacting && reactor.is_franchised() && user.id != message.author {
                    self.adjust_contribution(message.author, message.server_id, -1).await;
                }
            }
        }
        Ok(())
    }

    // --- internals -------------------------------------------------------

    async fn resolve_poster(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<(domain::User, domain::Server, Channel, domain::UserId), MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let membership = match self.memberships.get(user.id, server.id).await? {
            None => return Err(MessageError::NotAMember(handle.to_string())),
            Some(m) if m.is_sanctioned => return Err(MessageError::Sanctioned(handle.to_string())),
            Some(m) => m,
        };
        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name).await?
            .ok_or_else(|| MessageError::NoSuchChannel(channel_name.to_string()))?;
        if channel.visibility.is_appeals() {
            // The appeals room: only voters, police, and muted appellants may post —
            // and a muted appellant *may* post here, that being the whole point. To
            // anyone else the channel does not exist.
            if !crate::mute_service::membership_sees_appeals(&membership) {
                return Err(MessageError::NoSuchChannel(channel_name.to_string()));
            }
        } else if membership.is_muted {
            // Muted everywhere else.
            return Err(MessageError::Muted(handle.to_string()));
        }
        let uid = user.id;
        Ok((user, server, channel, uid))
    }

    async fn insert_message(
        &self,
        author: domain::UserId,
        channel: &Channel,
        body: &str,
        parent: Option<domain::MessageId>,
        attachments: Vec<domain::Attachment>,
    ) -> Result<Message, MessageError> {
        // A plaintext body may not enter an encrypted channel — the client must seal
        // it and use `post_sealed_message`. Media is plaintext-only for now, so it
        // rides this same guard.
        if channel.is_encrypted {
            return Err(MessageError::ChannelEncrypted);
        }
        let body = body.trim();
        // A message must carry *something* — text or at least one attachment.
        if body.is_empty() && attachments.is_empty() {
            return Err(MessageError::EmptyBody);
        }
        let mut message = Message::new(
            self.messages.next_message_id().await?,
            channel.id,
            channel.server_id,
            author,
            body,
            parent,
            self.clock.now(),
        );
        message.attachments = attachments;
        self.messages.insert_message(message.clone()).await?;
        Ok(message)
    }

    async fn authored_message(&self, handle: &str, message_id: u64) -> Result<Message, MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id)).await?
            .ok_or(MessageError::NoSuchMessage(message_id))?;
        if message.author != user.id {
            return Err(MessageError::NotTheAuthor);
        }
        Ok(message)
    }

    async fn adjust_contribution(&self, author: domain::UserId, server: domain::ServerId, delta: i64) {
        if let Some(mut m) = self.memberships.get(author, server).await.ok().flatten() {
            m.contribution = (m.contribution + delta).max(0);
            self.memberships.upsert(m).await.unwrap_or_default();
        }
    }
}
