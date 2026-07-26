//! Use-cases for custom emoji: add (citizen), vote (citizen), and the ranked list.
//!
//! Emoji are curated by continuous voting rather than a governance ballot. Only
//! **currently-franchised citizens** may add or vote, and only their votes are
//! tallied — the same active-member tier that runs the rest of the democracy.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use domain::{normalize_emoji_name, rank_emojis, Emoji, EmojiId, EmojiVote, UserId};

use crate::emoji_view::RankedEmoji;
use crate::{Clock, EmojiError, EmojiStore, EmojiVoteStore, MembershipStore, ServerStore, UserStore};

/// Custom-emoji use-cases held on their own handle, reached via [`Services::emoji`].
#[derive(Clone)]
pub struct EmojiService {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) emoji_votes: Arc<dyn EmojiVoteStore>,
    pub(crate) emojis: Arc<dyn EmojiStore>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

impl EmojiService {
    /// Add a custom emoji to a server's pool (citizen-only). `url` is the final
    /// image reference — a `data:` URI the web layer already validated, or an
    /// external URL. The name is normalized and must be unique on the server.
    pub async fn add_emoji(
        &self,
        adder_handle: &str,
        server_slug: &str,
        name: &str,
        url: &str,
    ) -> Result<Emoji, EmojiError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| EmojiError::NoSuchServer(server_slug.to_string()))?;
        let user = self
            .users
            .find_by_handle(adder_handle.trim()).await?
            .ok_or_else(|| EmojiError::NoSuchUser(adder_handle.to_string()))?;
        self.memberships
            .get(user.id, server.id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(EmojiError::NotACitizen)?;

        let name = normalize_emoji_name(name);
        if name.is_empty() {
            return Err(EmojiError::BadName);
        }
        if self.emojis.find_emoji(server.id, &name).await?.is_some() {
            return Err(EmojiError::NameTaken(name));
        }
        let emoji = Emoji::new(
            self.emojis.next_emoji_id().await?,
            server.id,
            name,
            url,
            user.id,
            self.clock.now(),
        );
        self.emojis.insert_emoji(emoji.clone()).await?;
        Ok(emoji)
    }

    /// Cast (or change) a citizen's up/down vote on an emoji.
    pub async fn vote_emoji(
        &self,
        voter_handle: &str,
        server_slug: &str,
        emoji_id: u64,
        is_up: bool,
    ) -> Result<(), EmojiError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| EmojiError::NoSuchServer(server_slug.to_string()))?;
        let user = self
            .users
            .find_by_handle(voter_handle.trim()).await?
            .ok_or_else(|| EmojiError::NoSuchUser(voter_handle.to_string()))?;
        self.memberships
            .get(user.id, server.id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(EmojiError::NotACitizen)?;

        let emoji = self
            .emojis
            .get_emoji(EmojiId(emoji_id)).await?
            .filter(|e| e.server_id == server.id)
            .ok_or(EmojiError::NoSuchEmoji(emoji_id))?;
        self.emoji_votes
            .upsert_emoji_vote(EmojiVote::new(server.id, emoji.id, user.id, is_up)).await?;
        Ok(())
    }

    /// The server's vote list: **active + considered** emoji, highest net score
    /// first. Only currently-franchised citizens' votes are counted, so a revoked
    /// franchise silently stops counting without touching stored votes.
    pub async fn ranked_emojis(&self, server_slug: &str, viewer_handle: &str) -> Vec<RankedEmoji> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return Vec::new();
        };
        // Oldest first, so equal scores rank by age (stable, older wins).
        let mut emojis = self.emojis.list_for_server(server.id).await.unwrap_or_default();
        emojis.sort_by_key(|e| e.id.0);

        let citizens: HashSet<UserId> = self
            .memberships
            .list_for_server(server.id).await.unwrap_or_default()
            .into_iter()
            .filter(|m| m.is_franchised(self.clock.now()))
            .map(|m| m.user_id)
            .collect();
        let mut score: HashMap<EmojiId, i64> = HashMap::new();
        for v in self.emoji_votes.emoji_votes_for_server(server.id).await.unwrap_or_default() {
            if citizens.contains(&v.voter) {
                *score.entry(v.emoji_id).or_default() += if v.is_up { 1 } else { -1 };
            }
        }

        let scores: Vec<i64> = emojis.iter().map(|e| *score.get(&e.id).unwrap_or(&0)).collect();
        let standings = rank_emojis(&scores);
        let viewer = self.users.find_by_handle(viewer_handle.trim()).await.ok().flatten().map(|u| u.id);

        let mut items: Vec<RankedEmoji> = Vec::new();
        for (i, e) in emojis.iter().enumerate() {
            if !standings[i].is_listed() {
                continue;
            }
            let my_vote = match viewer {
                Some(v) => self.emoji_votes.my_emoji_vote(e.id, v).await.ok().flatten(),
                None => None,
            };
            items.push(RankedEmoji {
                id: e.id.0,
                name: e.name.clone(),
                url: e.url.clone(),
                score: scores[i],
                standing: standings[i],
                my_vote,
            });
        }
        // Present highest score first; ties by age (older id first).
        items.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        items
    }

    /// Every retained emoji's `name → url` for a server — including archived ones —
    /// so the client can render `:name:` in any message, however old.
    pub async fn emoji_name_map(&self, server_slug: &str) -> Vec<(String, String)> {
        match self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() {
            Some(s) => self
                .emojis
                .list_for_server(s.id).await.unwrap_or_default()
                .into_iter()
                .map(|e| (e.name, e.url))
                .collect(),
            None => Vec::new(),
        }
    }
}
