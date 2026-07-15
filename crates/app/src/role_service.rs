//! Role use-cases: read custom roles and their holders, and resolve the
//! `@mentions` in a message body.
//!
//! Roles are created and populated **only by ballot** (see the `AssignRole` /
//! `CreateRole` arms of `apply_effect`), so there is deliberately no `assign` or
//! `create` method here — those are effects of governance, not direct calls. What
//! lives here is read-side: listing roles for a picker, and turning the tokens in
//! a message into the members they address.

use std::collections::BTreeSet;

use domain::{parse_mentions, Role, ServerId, StandingRole};

use crate::mention::{MentionKind, ResolvedMention};
use crate::Services;

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
