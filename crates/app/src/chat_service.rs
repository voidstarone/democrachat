//! Chat use-cases: channels, messages, threaded replies, and reactions.
//!
//! The one governance touch-point here is **who may create a channel**: in Seed
//! the founder provisions the server; past Seed that becomes a ballot (M3). The
//! payoff of this milestone is that a **citizen's reaction endorses** a message's
//! author — the first real driver of the `contribution` score that Layer 1 of the
//! franchise consumes. An endorsement counts once per (message, citizen),
//! regardless of how many emojis they pile on, and is withdrawn if they clear
//! their reactions.

use domain::{Channel, Message, Phase, Reaction};

use crate::{ChannelError, MessageError, ReactionError, Services};

/// Per-file upload cap for a media attachment (25 MiB).
const MAX_MEDIA_BYTES: usize = 25 * 1024 * 1024;

impl Services {
    /// Create a channel. Allowed only while the server is in **Seed** and only by
    /// its founder (provisioning) — a larger server governs its channels by ballot.
    pub fn create_channel(
        &self,
        founder_handle: &str,
        server_slug: &str,
        name: &str,
        topic: &str,
    ) -> Result<Channel, ChannelError> {
        let user = self
            .users
            .find_by_handle(founder_handle.trim())
            .ok_or_else(|| ChannelError::NoSuchUser(founder_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| ChannelError::NoSuchServer(server_slug.to_string()))?;

        let citizens = self.memberships.citizen_count(server.id);
        let phase = Phase::from_citizen_count(citizens);
        if !(phase.founder_may_provision() && user.id == server.founder_id) {
            return Err(ChannelError::NotProvisionable);
        }

        let name = domain::normalize_channel_name(name);
        if name.is_empty() {
            return Err(ChannelError::EmptyName);
        }
        if self.channels.find_by_name(server.id, &name).is_some() {
            return Err(ChannelError::NameTaken(name));
        }

        let channel = Channel::new(
            self.channels.next_channel_id(),
            server.id,
            name,
            topic.trim(),
            self.clock.now(),
        );
        self.channels.insert_channel(channel.clone());
        Ok(channel)
    }

    /// List a server's channels (in id order). `None` if no such server.
    pub fn list_channels(&self, server_slug: &str) -> Option<Vec<Channel>> {
        let server = self.servers.find_by_slug(server_slug.trim())?;
        Some(self.channels.list_for_server(server.id))
    }

    /// Read-only: a user's handle by id (for rendering authorship).
    pub fn user_handle(&self, id: domain::UserId) -> Option<String> {
        self.users.get_user(id).map(|u| u.handle)
    }

    /// Read-only: a user's id by handle. The federation vote router needs the
    /// numeric id (handles are for humans; the wire carries ids).
    pub fn user_id(&self, handle: &str) -> Option<u64> {
        self.users.find_by_handle(handle.trim()).map(|u| u.id.0)
    }

    /// Read-only: a message's reactions, summarized per emoji in stable order.
    pub fn message_reactions(&self, message_id: u64) -> Vec<(String, u64)> {
        let rs = self.reactions.list_for_message(domain::MessageId(message_id));
        domain::summarize_reactions(&rs)
    }

    /// Read-only: which (server slug, channel name) a message lives in — so a
    /// realtime event can be routed to the right channel view.
    pub fn message_context(&self, message_id: u64) -> Option<(String, String)> {
        let m = self.messages.get_message(domain::MessageId(message_id))?;
        let server = self.servers.get_server(m.server_id)?;
        let channel = self.channels.get_channel(m.channel_id)?;
        Some((server.slug, channel.name))
    }

    /// Post a top-level message to a channel. Requires server membership.
    pub fn post_message(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        body: &str,
    ) -> Result<Message, MessageError> {
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name)?;
        self.insert_message(author_id, &channel, body, None, Vec::new())
    }

    /// Post a plaintext message with media attachments. The blobs must already be in
    /// the media store (via [`store_media`](Self::store_media)); this only records
    /// the references on the message. Media-only messages (empty body) are allowed.
    pub fn post_message_with_attachments(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        body: &str,
        attachments: Vec<domain::Attachment>,
    ) -> Result<Message, MessageError> {
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name)?;
        self.insert_message(author_id, &channel, body, None, attachments)
    }

    /// Validate and store an uploaded media blob, returning its storage key and
    /// derived [`MediaKind`]. Rejects empty uploads, oversized files, and any MIME
    /// type that is not image/video/audio. The bytes go to the media store (a
    /// separate storage tier), keyed opaquely.
    pub fn store_media(
        &self,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(String, domain::MediaKind), crate::MediaError> {
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
        if base == "image/svg+xml" {
            return Err(crate::MediaError::UnsupportedType(base));
        }
        let kind = domain::MediaKind::from_content_type(&base)
            .ok_or_else(|| crate::MediaError::UnsupportedType(base.clone()))?;
        let key = self.media.put(&base, bytes)?;
        Ok((key, kind))
    }

    /// Fetch a stored media blob and its content type for serving.
    pub fn media_blob(&self, key: &str) -> Option<(String, Vec<u8>)> {
        self.media.get(key)
    }

    /// Post an **end-to-end-encrypted** message: the client has already sealed the
    /// body under the channel key of `key_epoch`, so `ciphertext` is opaque to the
    /// server. Only valid in an encrypted channel; requires membership. An optional
    /// `parent` threads the reply (it must live in the same channel).
    pub fn post_sealed_message(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
        ciphertext: &str,
        key_epoch: u32,
        parent: Option<u64>,
    ) -> Result<Message, MessageError> {
        let (_user, _server, channel, author_id) =
            self.resolve_poster(handle, server_slug, channel_name)?;
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
                    .get_message(domain::MessageId(pid))
                    .ok_or(MessageError::NoSuchMessage(pid))?;
                if p.channel_id != channel.id {
                    return Err(MessageError::CrossChannelReply);
                }
                Some(p.id)
            }
            None => None,
        };
        let message = Message::sealed(
            self.messages.next_message_id(),
            channel.id,
            channel.server_id,
            author_id,
            ciphertext,
            key_epoch,
            parent,
            self.clock.now(),
        );
        self.messages.insert_message(message.clone());
        Ok(message)
    }

    /// Reply to an existing message, threading under it. Requires membership.
    pub fn reply_message(
        &self,
        handle: &str,
        parent_id: u64,
        body: &str,
    ) -> Result<Message, MessageError> {
        let parent = self
            .messages
            .get_message(domain::MessageId(parent_id))
            .ok_or(MessageError::NoSuchMessage(parent_id))?;
        let channel = self
            .channels
            .get_channel(parent.channel_id)
            .ok_or_else(|| MessageError::NoSuchChannel(parent.channel_id.to_string()))?;
        let server = self
            .servers
            .get_server(parent.server_id)
            .ok_or_else(|| MessageError::NoSuchServer(parent.server_id.to_string()))?;

        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        match self.memberships.get(user.id, server.id) {
            None => return Err(MessageError::NotAMember(handle.to_string())),
            Some(m) if m.is_sanctioned => return Err(MessageError::Sanctioned(handle.to_string())),
            Some(_) => {}
        }
        self.insert_message(user.id, &channel, body, Some(parent.id), Vec::new())
    }

    /// Edit a message's body. Author only.
    pub fn edit_message(
        &self,
        handle: &str,
        message_id: u64,
        body: &str,
    ) -> Result<Message, MessageError> {
        let body = body.trim();
        if body.is_empty() {
            return Err(MessageError::EmptyBody);
        }
        let mut message = self.authored_message(handle, message_id)?;
        message.edit(body, self.clock.now());
        self.messages.update_message(message.clone());
        Ok(message)
    }

    /// Delete (tombstone) a message. Author only. Replies keep their place.
    pub fn delete_message(&self, handle: &str, message_id: u64) -> Result<(), MessageError> {
        let mut message = self.authored_message(handle, message_id)?;
        // Media lives and dies with its message: drop the blobs before tombstoning
        // (which clears the attachment list).
        for a in &message.attachments {
            self.media.delete(&a.key);
        }
        message.tombstone();
        self.messages.update_message(message);
        Ok(())
    }

    /// The messages of a channel, flat and in post order, for the caller to build
    /// into a thread tree.
    pub fn channel_messages(
        &self,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<Vec<Message>, MessageError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name)
            .ok_or_else(|| MessageError::NoSuchChannel(channel_name.to_string()))?;
        Ok(self.messages.list_for_channel(channel.id))
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
    pub fn channel_messages_for(
        &self,
        viewer_handle: &str,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<Vec<Message>, MessageError> {
        let all = self.channel_messages(server_slug, channel_name)?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let viewer_id = self.users.find_by_handle(viewer_handle.trim()).map(|u| u.id);
        let viewer_joined = viewer_id
            .and_then(|id| self.memberships.get(id, server.id))
            .map(|m| m.joined_at)
            .unwrap_or_else(|| self.clock.now());
        let mut author_cache: std::collections::HashMap<domain::UserId, Option<domain::Membership>> =
            std::collections::HashMap::new();
        Ok(all
            .into_iter()
            .filter(|m| {
                if viewer_id == Some(m.author) {
                    return true; // you always see your own messages
                }
                let author = author_cache
                    .entry(m.author)
                    .or_insert_with(|| self.memberships.get(m.author, server.id));
                match author {
                    Some(a) => a.shows_message_to(m.created_at, viewer_joined),
                    None => true, // author left/unknown → nothing to hide
                }
            })
            .collect())
    }

    /// This member's personal history-sharing preference for a server, if they are
    /// a member. `true` (share with newcomers) is the default.
    pub fn history_sharing(&self, handle: &str, server_slug: &str) -> Option<bool> {
        let user = self.users.find_by_handle(handle.trim())?;
        let server = self.servers.find_by_slug(server_slug.trim())?;
        self.memberships
            .get(user.id, server.id)
            .map(|m| m.shares_history_with_newcomers)
    }

    /// Set the caller's personal history-sharing preference for a server: whether
    /// members who join *after* their messages may read them. A personal, per-server
    /// control — it never affects members already present when a message was posted.
    pub fn set_history_sharing(
        &self,
        handle: &str,
        server_slug: &str,
        shares: bool,
    ) -> Result<(), MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        let mut m = self
            .memberships
            .get(user.id, server.id)
            .ok_or_else(|| MessageError::NotAMember(handle.to_string()))?;
        m.shares_history_with_newcomers = shares;
        self.memberships.upsert(m);
        Ok(())
    }

    /// React to a message with an emoji. Requires membership. A **citizen's**
    /// reaction endorses the author, raising their contribution once per
    /// (message, citizen).
    pub fn react(
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
            .find_by_handle(handle.trim())
            .ok_or_else(|| ReactionError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id))
            .ok_or(ReactionError::NoSuchMessage(message_id))?;
        let reactor = self
            .memberships
            .get(user.id, message.server_id)
            .ok_or_else(|| ReactionError::NotAMember(handle.to_string()))?;

        // Endorsement: a franchised citizen reacting (for the first time) to
        // someone else's message raises that author's contribution by one.
        let first_reaction = !self.reactions.user_has_any(message.id, user.id);
        let endorses = first_reaction && reactor.is_franchised() && user.id != message.author;

        let added = self.reactions.add(Reaction::new(message.id, user.id, emoji));
        if added && endorses {
            self.adjust_contribution(message.author, message.server_id, 1);
        }
        Ok(())
    }

    /// Remove a reaction. If it was a citizen's sole reaction on the message, the
    /// endorsement is withdrawn.
    pub fn unreact(
        &self,
        handle: &str,
        message_id: u64,
        emoji: &str,
    ) -> Result<(), ReactionError> {
        let emoji = emoji.trim();
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| ReactionError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id))
            .ok_or(ReactionError::NoSuchMessage(message_id))?;

        let removed = self.reactions.remove(message.id, user.id, emoji);
        if removed {
            let still_reacting = self.reactions.user_has_any(message.id, user.id);
            if let Some(reactor) = self.memberships.get(user.id, message.server_id) {
                if !still_reacting && reactor.is_franchised() && user.id != message.author {
                    self.adjust_contribution(message.author, message.server_id, -1);
                }
            }
        }
        Ok(())
    }

    // --- internals -------------------------------------------------------

    fn resolve_poster(
        &self,
        handle: &str,
        server_slug: &str,
        channel_name: &str,
    ) -> Result<(domain::User, domain::Server, Channel, domain::UserId), MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| MessageError::NoSuchServer(server_slug.to_string()))?;
        match self.memberships.get(user.id, server.id) {
            None => return Err(MessageError::NotAMember(handle.to_string())),
            Some(m) if m.is_sanctioned => return Err(MessageError::Sanctioned(handle.to_string())),
            Some(_) => {}
        }
        let name = domain::normalize_channel_name(channel_name);
        let channel = self
            .channels
            .find_by_name(server.id, &name)
            .ok_or_else(|| MessageError::NoSuchChannel(channel_name.to_string()))?;
        let uid = user.id;
        Ok((user, server, channel, uid))
    }

    fn insert_message(
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
            self.messages.next_message_id(),
            channel.id,
            channel.server_id,
            author,
            body,
            parent,
            self.clock.now(),
        );
        message.attachments = attachments;
        self.messages.insert_message(message.clone());
        Ok(message)
    }

    fn authored_message(&self, handle: &str, message_id: u64) -> Result<Message, MessageError> {
        let user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| MessageError::NoSuchUser(handle.to_string()))?;
        let message = self
            .messages
            .get_message(domain::MessageId(message_id))
            .ok_or(MessageError::NoSuchMessage(message_id))?;
        if message.author != user.id {
            return Err(MessageError::NotTheAuthor);
        }
        Ok(message)
    }

    fn adjust_contribution(&self, author: domain::UserId, server: domain::ServerId, delta: i64) {
        if let Some(mut m) = self.memberships.get(author, server) {
            m.contribution = (m.contribution + delta).max(0);
            self.memberships.upsert(m);
        }
    }
}
