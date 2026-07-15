//! Policing use-cases: a **police** officer instantly muting or unmuting a member,
//! plus the read-side helpers the web layer needs (member roster, a caller's
//! police/mute status, and who may see the restricted `#appeals` channel).
//!
//! Appointing and dismissing police, and imposing/lifting mutes *by vote*, are all
//! governance effects — they live in `governance_service`'s `apply_kind`. What
//! lives here is the **instant** power a sitting officer wields directly, and the
//! access rules for the appeals channel.

use std::sync::Arc;

use domain::Membership;

use crate::{Clock, MemberView, MembershipStore, MuteError, ServerStore, UserStore};

/// Policing use-cases held on their own handle, reached via [`Services::mute`].
#[derive(Clone)]
pub struct MuteService {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
}

impl MuteService {
    /// Instantly mute a member. Gated on: the actor is a police officer of the
    /// server, the target is an ordinary member (police cannot be muted, nor can an
    /// officer mute themselves), and no vote-imposed 24-hour cooldown bars *this*
    /// officer from re-muting *this* member.
    pub fn mute_member(
        &self,
        officer_handle: &str,
        server_slug: &str,
        target_handle: &str,
    ) -> Result<(), MuteError> {
        let (officer, server) = self.resolve_officer(officer_handle, server_slug)?;
        let target = self
            .users
            .find_by_handle(target_handle.trim())?
            .ok_or_else(|| MuteError::NotAMember(target_handle.to_string()))?;
        let mut m = self
            .memberships
            .get(target.id, server.id)?
            .ok_or_else(|| MuteError::NotAMember(target_handle.to_string()))?;
        if m.is_police {
            return Err(MuteError::CannotMutePolice);
        }
        let now = self.clock.now();
        if m.is_remute_blocked_for(officer.id, now) {
            return Err(MuteError::RemuteBlocked);
        }
        m.mute(Some(officer.id));
        self.memberships.upsert(m)?;
        Ok(())
    }

    /// Instantly lift a member's mute. Any police officer may do this; a
    /// police-lifted mute carries no re-mute cooldown (only a *vote* lift does).
    pub fn unmute_member(
        &self,
        officer_handle: &str,
        server_slug: &str,
        target_handle: &str,
    ) -> Result<(), MuteError> {
        let (_officer, server) = self.resolve_officer(officer_handle, server_slug)?;
        let target = self
            .users
            .find_by_handle(target_handle.trim())?
            .ok_or_else(|| MuteError::NotAMember(target_handle.to_string()))?;
        let mut m = self
            .memberships
            .get(target.id, server.id)?
            .ok_or_else(|| MuteError::NotAMember(target_handle.to_string()))?;
        m.unmute(false, now_placeholder());
        self.memberships.upsert(m)?;
        Ok(())
    }

    /// Resolve the acting officer and server, checking the officer holds the police
    /// power. Shared by mute/unmute.
    fn resolve_officer(&self, officer_handle: &str, server_slug: &str) -> Result<(domain::User, domain::Server), MuteError> {
        let officer = self
            .users
            .find_by_handle(officer_handle.trim())?
            .ok_or_else(|| MuteError::NoSuchUser(officer_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())?
            .ok_or_else(|| MuteError::NoSuchServer(server_slug.to_string()))?;
        let is_police = self
            .memberships
            .get(officer.id, server.id)?
            .is_some_and(|m| m.is_police);
        if !is_police {
            return Err(MuteError::NotPolice);
        }
        Ok((officer, server))
    }

    /// Read-only: every member of a server with the flags a roster needs (for the
    /// ban picker, the police moderation panel, etc.), in join order.
    pub fn list_members(&self, server_slug: &str) -> Vec<MemberView> {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).ok().flatten() else {
            return Vec::new();
        };
        self.memberships
            .list_for_server(server.id).unwrap_or_default()
            .into_iter()
            .filter_map(|m| {
                self.users.get_user(m.user_id).ok().flatten().map(|u| MemberView {
                    handle: u.handle,
                    tier: m.tier,
                    is_sanctioned: m.is_sanctioned,
                    is_muted: m.is_muted,
                    is_police: m.is_police,
                })
            })
            .collect()
    }

    /// Read-only: a caller's `(is_police, is_muted)` on a server, if a member.
    pub fn police_and_mute_status(&self, handle: &str, server_slug: &str) -> Option<(bool, bool)> {
        let user = self.users.find_by_handle(handle.trim()).ok().flatten()?;
        let server = self.servers.find_by_slug(server_slug.trim()).ok().flatten()?;
        self.memberships.get(user.id, server.id).ok().flatten().map(|m| (m.is_police, m.is_muted))
    }
}

/// The pure predicate behind [`Services::appeals_visible_to`].
pub(crate) fn membership_sees_appeals(m: &Membership) -> bool {
    m.is_citizen() || m.is_muted || m.is_police
}

/// `unmute(by_vote=false, …)` ignores its cooldown argument; this names that.
fn now_placeholder() -> domain::Timestamp {
    domain::Timestamp(0)
}
