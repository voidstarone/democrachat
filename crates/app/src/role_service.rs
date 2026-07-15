//! Role use-cases: read custom roles and their holders, and resolve the
//! `@mentions` in a message body.
//!
//! A role's *existence* is decided by ballot (the `CreateRole` / `DeleteRole` arms
//! of `apply_effect`); its *membership* is not decided at all — it is **derived**
//! on read by evaluating each member against the role's
//! [`RoleCriteria`](domain::RoleCriteria). So there is no `assign` method here; a
//! holder is simply a member who currently qualifies (and, for `@moderator`, has
//! not opted out). What lives here is read-side: listing roles, computing their
//! current holders, and turning the tokens in a message into the members they
//! address.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use domain::{
    parse_mentions, winning_color, Role, RoleColor, RoleColorVote, RoleId, ServerId, StandingRole,
    UserId,
};

use crate::{MentionKind, ResolvedMention, RoleColorView, UserRoles};
use crate::{
    Clock, MembershipStore, RoleColorVoteStore, RoleError, RoleStore, ServerStore, UserStore,
};

/// Role use-cases held on their own handle, reached via [`Services::roles`].
#[derive(Clone)]
pub struct RoleService {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) role_color_votes: Arc<dyn RoleColorVoteStore>,
    pub(crate) roles: Arc<dyn RoleStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

impl RoleService {
    /// Read-only: the handles of a server's members (any tier), for `@mention`
    /// autocomplete.
    pub async fn member_handles(&self, server_slug: &str) -> Vec<String> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for m in self.memberships.list_for_server(server.id).await.unwrap_or_default() {
            if let Some(u) = self.users.get_user(m.user_id).await.ok().flatten() {
                out.push(u.handle);
            }
        }
        out
    }

    /// Read-only: a server's custom roles, in creation order.
    pub async fn list_roles(&self, server_slug: &str) -> Vec<Role> {
        match self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() {
            Some(s) => self.roles.list_for_server(s.id).await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Read-only: the handles holding a custom role (by role name) in a server.
    pub async fn role_holders(&self, server_slug: &str, role_name: &str) -> Vec<String> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return Vec::new();
        };
        let name = domain::normalize_role_name(role_name);
        let Some(role) = self.roles.find_role(server.id, &name).await.ok().flatten() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for id in self.holders_of(&role).await {
            if let Some(u) = self.users.get_user(id).await.ok().flatten() {
                out.push(u.handle);
            }
        }
        out
    }

    /// Read-only: a server's custom roles, each with its current (plurality)
    /// colour among franchised citizens. Powers the coloured role chips.
    pub async fn roles_with_color(&self, server_slug: &str) -> Vec<(Role, Option<RoleColor>)> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return Vec::new();
        };
        let franchised = self.franchised_set(server.id).await;
        let votes = self.role_color_votes.role_color_votes_for_server(server.id).await.unwrap_or_default();
        self.roles
            .list_for_server(server.id).await.unwrap_or_default()
            .into_iter()
            .map(|r| {
                let color = winning_color(
                    votes
                        .iter()
                        .filter(|v| v.role_id == r.id && franchised.contains(&v.voter))
                        .map(|v| &v.color),
                );
                (r, color)
            })
            .collect()
    }

    /// Read-only: the roles a member holds on a server, for the identity popover —
    /// the standing roles their tier admits plus every custom role assigned to
    /// them, each with its voted colour and the viewer's own colour vote. `None`
    /// if the target isn't a member here.
    pub async fn user_roles(&self, server_slug: &str, handle: &str, viewer_handle: &str) -> Option<UserRoles> {
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        let membership = self.memberships.get(user.id, server.id).await.ok().flatten()?;

        let standing = StandingRole::all()
            .into_iter()
            .filter(|s| s.admits(membership.tier))
            .map(|s| s.name().to_string())
            .collect();

        let franchised = self.franchised_set(server.id).await;
        let votes = self.role_color_votes.role_color_votes_for_server(server.id).await.unwrap_or_default();
        let viewer = self.users.find_by_handle(viewer_handle.trim()).await.ok().flatten().map(|u| u.id);

        let mut roles = Vec::new();
        for r in self.roles.list_for_server(server.id).await.unwrap_or_default() {
            if !self.holders_of(&r).await.contains(&user.id) {
                continue;
            }
            let color = winning_color(
                votes
                    .iter()
                    .filter(|v| v.role_id == r.id && franchised.contains(&v.voter))
                    .map(|v| &v.color),
            );
            let my_color = match viewer {
                Some(vid) => self
                    .role_color_votes
                    .my_role_color_vote(r.id, vid).await.ok().flatten()
                    .map(|c| c.as_str().to_string()),
                None => None,
            };
            roles.push(RoleColorView {
                id: r.id.0,
                name: r.name,
                color: color.map(|c| c.as_str().to_string()),
                my_color,
            });
        }

        Some(UserRoles { handle: user.handle, tier: membership.tier, standing, roles })
    }

    /// Cast (or change) a citizen's vote for a custom role's colour. A continuous
    /// plurality vote (like emoji), not a ballot: only franchised citizens count,
    /// and the winning colour is recomputed live from every current vote.
    pub async fn vote_role_color(
        &self,
        voter_handle: &str,
        server_slug: &str,
        role_id: u64,
        color: &str,
    ) -> Result<(), RoleError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| RoleError::NoSuchServer(server_slug.to_string()))?;
        let user = self
            .users
            .find_by_handle(voter_handle.trim()).await?
            .ok_or_else(|| RoleError::NoSuchUser(voter_handle.to_string()))?;
        self.memberships
            .get(user.id, server.id).await?
            .filter(|m| m.is_franchised())
            .ok_or(RoleError::NotACitizen)?;

        let color = RoleColor::parse(color).ok_or(RoleError::BadColor)?;
        let role = self
            .roles
            .get_role(RoleId(role_id)).await?
            .filter(|r| r.server_id == server.id)
            .ok_or(RoleError::NoSuchRole(role_id))?;
        self.role_color_votes
            .upsert_role_color_vote(RoleColorVote::new(server.id, role.id, user.id, color)).await?;
        Ok(())
    }

    /// Whether the caller has opted **out** of the `@moderator` role on a server,
    /// if they are a member. `false` (the default) means they hold moderator
    /// whenever they meet its criteria; `true` means they never do.
    pub async fn moderator_optout(&self, handle: &str, server_slug: &str) -> Option<bool> {
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        let server = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten()?;
        self.memberships
            .get(user.id, server.id).await.ok().flatten()
            .map(|m| m.has_declined_moderator)
    }

    /// Set the caller's opt-out of the `@moderator` role on a server. The one
    /// self-serve role control: every other role is earned automatically by meeting
    /// its criteria with no way to refuse, but moderating is a duty, so a member may
    /// decline it (`declined: true`) and never hold it even while qualified.
    pub async fn set_moderator_optout(
        &self,
        handle: &str,
        server_slug: &str,
        declined: bool,
    ) -> Result<(), RoleError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| RoleError::NoSuchUser(handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| RoleError::NoSuchServer(server_slug.to_string()))?;
        let mut m = self
            .memberships
            .get(user.id, server.id).await?
            .ok_or_else(|| RoleError::NotAMember(handle.to_string()))?;
        m.has_declined_moderator = declined;
        self.memberships.upsert(m).await?;
        Ok(())
    }

    /// The user ids currently holding `role` — every member of the role's server
    /// who meets its [`RoleCriteria`](domain::RoleCriteria) right now, minus anyone
    /// who has declined it (moderator only). Derived, never stored: membership is a
    /// live function of standing, so it needs no assignment records and never goes
    /// stale. Returned in membership (join) order.
    async fn holders_of(&self, role: &Role) -> Vec<UserId> {
        let now = self.clock.now();
        let mut out = Vec::new();
        for m in self.memberships.list_for_server(role.server_id).await.unwrap_or_default() {
            if role.is_moderator() && m.has_declined_moderator {
                continue;
            }
            let Some(user) = self.users.get_user(m.user_id).await.ok().flatten() else {
                continue;
            };
            if role.criteria.admits(&user, &m, now) {
                out.push(m.user_id);
            }
        }
        out
    }

    /// The user ids of a server's currently-franchised citizens — the electorate
    /// whose votes are tallied.
    async fn franchised_set(&self, server: ServerId) -> HashSet<UserId> {
        self.memberships
            .list_for_server(server).await.unwrap_or_default()
            .into_iter()
            .filter(|m| m.is_franchised())
            .map(|m| m.user_id)
            .collect()
    }

    /// Read-only: the names a server offers for `@mention` autocomplete — every
    /// built-in standing role plus every custom role.
    pub async fn mentionable_role_names(&self, server_slug: &str) -> Vec<String> {
        let mut names: Vec<String> = StandingRole::all().iter().map(|r| r.name().to_string()).collect();
        names.extend(self.list_roles(server_slug).await.into_iter().map(|r| r.name));
        names
    }

    /// Resolve every `@mention` in `body` against a server: each token becomes a
    /// [`ResolvedMention`] naming what it is and the member handles it addresses.
    /// Unknown tokens are kept (as [`MentionKind::Unknown`]) so the caller can
    /// choose to render them plainly.
    pub async fn resolve_mentions(&self, server_slug: &str, body: &str) -> Vec<ResolvedMention> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return Vec::new();
        };

        let mut out = Vec::new();
        for token in parse_mentions(body) {
            out.push(self.resolve_one(server.id, token).await);
        }
        out
    }

    async fn resolve_one(&self, sid: ServerId, token: String) -> ResolvedMention {
        // 1. A built-in standing role (@everyone / @members / @citizens).
        if let Some(standing) = StandingRole::from_token(&token) {
            let handles = self.members_admitted_by(sid, standing).await;
            return ResolvedMention { token, kind: MentionKind::StandingRole, handles };
        }

        // 2. A custom, criteria-derived role.
        if let Some(role) = self.roles.find_role(sid, &token).await.ok().flatten() {
            let mut handles = Vec::new();
            for id in self.holders_of(&role).await {
                if let Some(u) = self.users.get_user(id).await.ok().flatten() {
                    handles.push(u.handle);
                }
            }
            return ResolvedMention { token, kind: MentionKind::Role, handles };
        }

        // 3. A member, by handle. Only counts if they belong to this server.
        if let Some(user) = self.users.find_by_handle(&token).await.ok().flatten() {
            if self.memberships.get(user.id, sid).await.ok().flatten().is_some() {
                return ResolvedMention {
                    token,
                    kind: MentionKind::User,
                    handles: vec![user.handle],
                };
            }
        }

        // 4. Nothing matched.
        ResolvedMention { token, kind: MentionKind::Unknown, handles: Vec::new() }
    }

    /// The handles of members whose tier a standing role addresses.
    async fn members_admitted_by(&self, sid: ServerId, standing: StandingRole) -> Vec<String> {
        let mut out = Vec::new();
        for m in self.memberships.list_for_server(sid).await.unwrap_or_default() {
            if !standing.admits(m.tier) {
                continue;
            }
            if let Some(u) = self.users.get_user(m.user_id).await.ok().flatten() {
                out.push(u.handle);
            }
        }
        out
    }

    /// Read-only: the distinct handles a message body pings on a server — the
    /// union of every resolved user/role mention. Deduplicated, order-stable.
    pub async fn mentioned_handles(&self, server_slug: &str, body: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for m in self.resolve_mentions(server_slug, body).await {
            for h in m.handles {
                if seen.insert(h.clone()) {
                    out.push(h);
                }
            }
        }
        out
    }
}
