//! Tag use-cases: label servers, channels, and users, and discover by tag.
//!
//! Tags are free-form discovery labels, stored on each entity as a single
//! pipe-fenced [`Tags`](domain::Tags) string so an exact-tag search is a fast
//! substring match (`|rust|`). Setting a server's or a channel's tags is a
//! founder-only edit (like the other Seed-era server settings); a user tags their
//! own account. Search is open to anyone — it is how the directory is browsed.

use std::sync::Arc;

use domain::{Channel, Server, Tags, User};

use crate::{ChannelStore, ServerStore, TagError, UserStore};

/// Tag use-cases held on their own handle, reached via [`Services::tags`].
#[derive(Clone)]
pub struct TagService {
    pub(crate) channels: Arc<dyn ChannelStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

impl TagService {
    /// Replace a server's tags. Founder-only: `editor` must be the server's founder.
    pub async fn set_server_tags(
        &self,
        editor: &str,
        slug: &str,
        raw: &str,
    ) -> Result<(), TagError> {
        let mut server = self
            .servers
            .find_by_slug(slug.trim()).await?
            .ok_or_else(|| TagError::NoSuchServer(slug.to_string()))?;
        self.require_founder(&server, editor).await?;
        server.tags = Tags::from_input(raw);
        self.servers.update_server(server).await?;
        Ok(())
    }

    /// Replace a channel's tags. Founder-only, keyed to the channel's own server.
    pub async fn set_channel_tags(
        &self,
        editor: &str,
        slug: &str,
        channel_name: &str,
        raw: &str,
    ) -> Result<(), TagError> {
        let server = self
            .servers
            .find_by_slug(slug.trim()).await?
            .ok_or_else(|| TagError::NoSuchServer(slug.to_string()))?;
        self.require_founder(&server, editor).await?;
        let mut channel = self
            .channels
            .find_by_name(server.id, channel_name.trim()).await?
            .ok_or_else(|| TagError::NoSuchChannel(channel_name.to_string()))?;
        channel.tags = Tags::from_input(raw);
        // `insert_channel` upserts, so this persists the edit in place.
        self.channels.insert_channel(channel).await?;
        Ok(())
    }

    /// Replace a user's own tags. The web layer authorizes that `handle` is the
    /// signed-in account (the same trust boundary as [`set_dm_policy`]).
    ///
    /// [`set_dm_policy`]: crate::SocialService::set_dm_policy
    pub async fn set_user_tags(&self, handle: &str, raw: &str) -> Result<(), TagError> {
        let mut user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| TagError::NoSuchUser(handle.to_string()))?;
        user.tags = Tags::from_input(raw);
        self.users.update_user(user).await?;
        Ok(())
    }

    /// Servers carrying `tag`, sorted by id. Empty when `tag` normalizes to nothing.
    pub async fn servers_with_tag(&self, tag: &str) -> Result<Vec<Server>, TagError> {
        Ok(self.servers.search_by_tag(tag).await?)
    }

    /// Channels carrying `tag` (across all servers), for discovery.
    pub async fn channels_with_tag(&self, tag: &str) -> Result<Vec<Channel>, TagError> {
        Ok(self.channels.search_by_tag(tag).await?)
    }

    /// Accounts carrying `tag`.
    pub async fn users_with_tag(&self, tag: &str) -> Result<Vec<User>, TagError> {
        Ok(self.users.search_by_tag(tag).await?)
    }

    /// Guard: only a server's founder may edit its (or its channels') tags.
    async fn require_founder(&self, server: &Server, editor: &str) -> Result<(), TagError> {
        let editor = self
            .users
            .find_by_handle(editor.trim()).await?
            .ok_or_else(|| TagError::NoSuchUser(editor.to_string()))?;
        if editor.id == server.founder_id {
            Ok(())
        } else {
            Err(TagError::Forbidden)
        }
    }
}

