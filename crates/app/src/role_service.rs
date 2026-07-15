//! Role use-cases: read custom roles and their holders, and resolve the
//! `@mentions` in a message body.
//!
//! Roles are created and populated **only by ballot** (see the `AssignRole` /
//! `CreateRole` arms of `apply_effect`), so there is deliberately no `assign` or
//! `create` method here — those are effects of governance, not direct calls. What
//! lives here is read-side: listing roles for a picker, and turning the tokens in
//! a message into the members they address.

use std::collections::{BTreeSet, HashSet};

use domain::{
    parse_mentions, winning_color, Role, RoleColor, RoleColorVote, RoleId, ServerId, StandingRole,
    UserId,
};

use crate::mention::{MentionKind, ResolvedMention};
use crate::role_view::{RoleColorView, UserRoles};
use crate::{RoleError, Services};

impl Services {
    /// Read-only: the handles of a server's members (any tier), for `@mention`
    /// autocomplete.
    pub fn member_handles(&self, server_slug: &str) -> Vec<String> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()) else {
            return Vec::new();
        };
        self.memberships
            .list_for_server(server.id)
            .into_iter()
            .filter_map(|m| self.users.get_user(m.user_id))
            .map(|u| u.handle)
            .collect()
    }

    /// Read-only: a server's custom roles, in creation order.
    pub fn list_roles(&self, server_slug: &str) -> Vec<Role> {
        match self.servers.find_by_slug(server_slug.trim()) {
            Some(s) => self.roles.list_for_server(s.id),
            None => Vec::new(),
        }
    }

    /// Read-only: the handles holding a custom role (by role name) in a server.
    pub fn role_holders(&self, server_slug: &str, role_name: &str) -> Vec<String> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()) else {
            return Vec::new();
        };
        let name = domain::normalize_role_name(role_name);
        let Some(role) = self.roles.find_role(server.id, &name) else {
            return Vec::new();
        };
        self.roles
            .holders(role.id)
            .into_iter()
            .filter_map(|id| self.users.get_user(id))
            .map(|u| u.handle)
            .collect()
    }

    /// Read-only: a server's custom roles, each with its current (plurality)
    /// colour among franchised citizens. Powers the coloured role chips.
    pub fn roles_with_color(&self, server_slug: &str) -> Vec<(Role, Option<RoleColor>)> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()) else {
            return Vec::new();
        };
        let franchised = self.franchised_set(server.id);
        let votes = self.role_color_votes.role_color_votes_for_server(server.id);
        self.roles
            .list_for_server(server.id)
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
    pub fn user_roles(&self, server_slug: &str, handle: &str, viewer_handle: &str) -> Option<UserRoles> {
        let server = self.servers.find_by_slug(server_slug.trim())?;
        let user = self.users.find_by_handle(handle.trim())?;
        let membership = self.memberships.get(user.id, server.id)?;

        let standing = StandingRole::all()
            .into_iter()
            .filter(|s| s.admits(membership.tier))
            .map(|s| s.name().to_string())
            .collect();

        let franchised = self.franchised_set(server.id);
        let votes = self.role_color_votes.role_color_votes_for_server(server.id);
        let viewer = self.users.find_by_handle(viewer_handle.trim()).map(|u| u.id);

        let roles = self
            .roles
            .list_for_server(server.id)
            .into_iter()
            .filter(|r| self.roles.holders(r.id).contains(&user.id))
            .map(|r| {
                let color = winning_color(
                    votes
                        .iter()
                        .filter(|v| v.role_id == r.id && franchised.contains(&v.voter))
                        .map(|v| &v.color),
                );
                let my_color = viewer
                    .and_then(|vid| self.role_color_votes.my_role_color_vote(r.id, vid))
                    .map(|c| c.as_str().to_string());
                RoleColorView {
                    id: r.id.0,
                    name: r.name,
                    color: color.map(|c| c.as_str().to_string()),
                    my_color,
                }
            })
            .collect();

        Some(UserRoles { handle: user.handle, tier: membership.tier, standing, roles })
    }

    /// Cast (or change) a citizen's vote for a custom role's colour. A continuous
    /// plurality vote (like emoji), not a ballot: only franchised citizens count,
    /// and the winning colour is recomputed live from every current vote.
    pub fn vote_role_color(
        &self,
        voter_handle: &str,
        server_slug: &str,
        role_id: u64,
        color: &str,
    ) -> Result<(), RoleError> {
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| RoleError::NoSuchServer(server_slug.to_string()))?;
        let user = self
            .users
            .find_by_handle(voter_handle.trim())
            .ok_or_else(|| RoleError::NoSuchUser(voter_handle.to_string()))?;
        self.memberships
            .get(user.id, server.id)
            .filter(|m| m.is_franchised())
            .ok_or(RoleError::NotACitizen)?;

        let color = RoleColor::parse(color).ok_or(RoleError::BadColor)?;
        let role = self
            .roles
            .get_role(RoleId(role_id))
            .filter(|r| r.server_id == server.id)
            .ok_or(RoleError::NoSuchRole(role_id))?;
        self.role_color_votes
            .upsert_role_color_vote(RoleColorVote::new(server.id, role.id, user.id, color));
        Ok(())
    }

    /// The user ids of a server's currently-franchised citizens — the electorate
    /// whose votes are tallied.
    fn franchised_set(&self, server: ServerId) -> HashSet<UserId> {
        self.memberships
            .list_for_server(server)
            .into_iter()
            .filter(|m| m.is_franchised())
            .map(|m| m.user_id)
            .collect()
    }

    /// Read-only: the names a server offers for `@mention` autocomplete — every
    /// built-in standing role plus every custom role.
    pub fn mentionable_role_names(&self, server_slug: &str) -> Vec<String> {
        let mut names: Vec<String> = StandingRole::all().iter().map(|r| r.name().to_string()).collect();
        names.extend(self.list_roles(server_slug).into_iter().map(|r| r.name));
        names
    }

    /// Resolve every `@mention` in `body` against a server: each token becomes a
    /// [`ResolvedMention`] naming what it is and the member handles it addresses.
    /// Unknown tokens are kept (as [`MentionKind::Unknown`]) so the caller can
    /// choose to render them plainly.
    pub fn resolve_mentions(&self, server_slug: &str, body: &str) -> Vec<ResolvedMention> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()) else {
            return Vec::new();
        };

        parse_mentions(body)
            .into_iter()
            .map(|token| self.resolve_one(server.id, token))
            .collect()
    }

    fn resolve_one(&self, sid: ServerId, token: String) -> ResolvedMention {
        // 1. A built-in standing role (@everyone / @members / @citizens).
        if let Some(standing) = StandingRole::from_token(&token) {
            let handles = self.members_admitted_by(sid, standing);
            return ResolvedMention { token, kind: MentionKind::StandingRole, handles };
        }

        // 2. A custom, ballot-created role.
        if let Some(role) = self.roles.find_role(sid, &token) {
            let handles = self
                .roles
                .holders(role.id)
                .into_iter()
                .filter_map(|id| self.users.get_user(id))
                .map(|u| u.handle)
                .collect();
            return ResolvedMention { token, kind: MentionKind::Role, handles };
        }

        // 3. A member, by handle. Only counts if they belong to this server.
        if let Some(user) = self.users.find_by_handle(&token) {
            if self.memberships.get(user.id, sid).is_some() {
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
    fn members_admitted_by(&self, sid: ServerId, standing: StandingRole) -> Vec<String> {
        self.memberships
            .list_for_server(sid)
            .into_iter()
            .filter(|m| standing.admits(m.tier))
            .filter_map(|m| self.users.get_user(m.user_id))
            .map(|u| u.handle)
            .collect()
    }

    /// Read-only: the distinct handles a message body pings on a server — the
    /// union of every resolved user/role mention. Deduplicated, order-stable.
    pub fn mentioned_handles(&self, server_slug: &str, body: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for m in self.resolve_mentions(server_slug, body) {
            for h in m.handles {
                if seen.insert(h.clone()) {
                    out.push(h);
                }
            }
        }
        out
    }
}
