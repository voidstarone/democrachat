//! Governance use-cases: open a proposal, cast a vote, and — once a ballot's
//! window closes — tally it, decide it, and **apply its effect** to the server.
//!
//! Everything here runs on the pure `domain` rules: the server's governance
//! surface decides *what* may be proposed, `threshold_for`/`decide` decide *how
//! hard* it is to pass, `Proposal::close` applies the phase threshold and the
//! constitutional timelock, and `VoteWeighting` decides how ballots are counted.
//! No effect is applied until a proposal has passed and matured.

use std::sync::Arc;

use domain::{
    Channel, DiscussionPost, Emoji, Phase, PhaseThresholds, Proposal, ProposalId, ProposalKind,
    ProposalStatus, Role, Rule, Server, ServerId, Tally, Timestamp, Vote,
};

use crate::{
    ChannelStore, Clock, EmojiStore, MembershipStore, ProposalStore, ProposeError, RoleStore,
    RuleStore, ServerStore, UserStore, VoteError, VoteStore,
};

/// Governance use-cases held on their own handle, reached via [`Services::governance`].
#[derive(Clone)]
pub struct GovernanceService {
    pub(crate) channels: Arc<dyn ChannelStore>,
    pub(crate) clock: Arc<dyn Clock>,
    /// Where this deployment's bootstrap phases begin. Set by
    /// [`Services::with_phase_thresholds`]; defaults to the platform thresholds.
    pub(crate) phase_thresholds: PhaseThresholds,
    pub(crate) emojis: Arc<dyn EmojiStore>,
    pub(crate) memberships: Arc<dyn MembershipStore>,
    pub(crate) proposals: Arc<dyn ProposalStore>,
    pub(crate) roles: Arc<dyn RoleStore>,
    pub(crate) rules: Arc<dyn RuleStore>,
    pub(crate) servers: Arc<dyn ServerStore>,
    pub(crate) users: Arc<dyn UserStore>,
    pub(crate) votes: Arc<dyn VoteStore>,
}

/// How long a freshly-opened ballot accepts votes before it can be closed.
const VOTING_WINDOW_HOURS: i64 = 48;

/// How long the clock is reset to when an amendment is folded in — the ballot has
/// changed, so voters get a fresh (shorter) window to weigh in on the new bundle.
const AMENDMENT_WINDOW_HOURS: i64 = 24;

impl GovernanceService {
    /// Open a proposal. Gated on: the proposer is an enfranchised citizen, the
    /// server governs this ballot kind (its surface), and the decision class is
    /// permitted in the server's current phase.
    pub async fn open_proposal(
        &self,
        proposer_handle: &str,
        server_slug: &str,
        kind: ProposalKind,
    ) -> Result<Proposal, ProposeError> {
        let user = self
            .users
            .find_by_handle(proposer_handle.trim()).await?
            .ok_or_else(|| ProposeError::NoSuchUser(proposer_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim()).await?
            .ok_or_else(|| ProposeError::NoSuchServer(server_slug.to_string()))?;

        // Only a franchised citizen may propose.
        self.memberships
            .get(user.id, server.id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(ProposeError::NotACitizen)?;

        // Surface + phase gates: the server must govern this kind, and its
        // decision class must be permitted in the current phase.
        self.ensure_ballot_admissible(&server, &kind).await?;

        let now = self.clock.now();
        let proposal = Proposal::new(
            self.proposals.next_proposal_id().await?,
            server.id,
            user.id,
            kind,
            now,
            now.plus_hours(VOTING_WINDOW_HOURS),
        );
        self.proposals.insert_proposal(proposal.clone()).await?;
        Ok(proposal)
    }

    /// Cast (or change) a vote on an open proposal. One ballot per citizen.
    pub async fn cast_vote(&self, voter_handle: &str, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let user = self
            .users
            .find_by_handle(voter_handle.trim()).await?
            .ok_or_else(|| VoteError::NoSuchUser(voter_handle.to_string()))?;
        self.cast_vote_as(user.id, proposal_id, is_aye).await
    }

    /// Cast a vote identified by the voter's **user id** rather than handle — the
    /// path a *forwarded* federation command takes, where the voter is a federated
    /// user the owner knows only by id (their home node authenticated them; the
    /// owner still re-checks citizenship here, never trusting the forwarder).
    pub async fn cast_vote_by_id(&self, voter_id: u64, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let user = self
            .users
            .get_user(domain::UserId(voter_id)).await?
            .ok_or_else(|| VoteError::NoSuchUser(voter_id.to_string()))?;
        self.cast_vote_as(user.id, proposal_id, is_aye).await
    }

    /// The shared core: validate the proposal is open and the voter is an
    /// enfranchised citizen of its server, then record the ballot. The two public
    /// entry points differ only in how they resolve the voter (handle vs id).
    async fn cast_vote_as(&self, voter_id: domain::UserId, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let proposal = self
            .proposals
            .get_proposal(ProposalId(proposal_id)).await?
            .ok_or(VoteError::NoSuchProposal(proposal_id))?;
        if proposal.status != ProposalStatus::Open {
            return Err(VoteError::Closed);
        }
        // Must be a franchised citizen of this server.
        self.memberships
            .get(voter_id, proposal.server_id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(VoteError::NotACitizen)?;

        self.votes.upsert_vote(Vote::new(proposal.id, voter_id, is_aye)).await?;
        Ok(())
    }

    /// The surface + phase gates shared by opening a proposal and amending one: the
    /// server must govern this ballot kind, and its decision class must be
    /// permitted in the server's current phase (e.g. no constitutional change in Seed).
    async fn ensure_ballot_admissible(&self, server: &Server, kind: &ProposalKind) -> Result<(), ProposeError> {
        if !server.governs(kind.ballot_kind()) {
            return Err(ProposeError::NotGoverned);
        }
        let citizens = self.memberships.citizen_count(server.id).await?;
        let phase = Phase::from_citizen_count(citizens, self.phase_thresholds);
        if domain::threshold_for(kind.decision_class(), phase).is_none() {
            return Err(ProposeError::NotAllowedInPhase);
        }
        Ok(())
    }

    /// Fold another change into an **open** proposal's bundle. The amendment shares
    /// the parent's single ballot: if it passes, every change — original and
    /// amendments — enacts together; if it fails, none do. Gated exactly like
    /// opening a proposal (franchised citizen, governed kind, permitted in phase),
    /// so an amendment can never smuggle in something the server doesn't vote on.
    pub async fn amend_proposal(
        &self,
        proposer_handle: &str,
        proposal_id: u64,
        kind: ProposalKind,
    ) -> Result<Proposal, ProposeError> {
        let user = self
            .users
            .find_by_handle(proposer_handle.trim()).await?
            .ok_or_else(|| ProposeError::NoSuchUser(proposer_handle.to_string()))?;
        let mut proposal = self
            .proposals
            .get_proposal(ProposalId(proposal_id)).await?
            .ok_or(ProposeError::NoSuchProposal(proposal_id))?;
        if proposal.status != ProposalStatus::Open {
            return Err(ProposeError::Closed);
        }
        let server = self
            .servers
            .get_server(proposal.server_id).await?
            .ok_or_else(|| ProposeError::NoSuchServer(proposal.server_id.0.to_string()))?;
        self.memberships
            .get(user.id, server.id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(ProposeError::NotACitizen)?;
        self.ensure_ballot_admissible(&server, &kind).await?;

        // The bundle people were voting on has changed: reset the deadline to a
        // fresh 24-hour window and nullify every vote already cast, so nobody is
        // recorded as endorsing an amendment they never saw.
        let now = self.clock.now();
        proposal.amend(kind, now.plus_hours(AMENDMENT_WINDOW_HOURS));
        self.votes.clear_for_proposal(proposal.id).await?;
        self.proposals.update_proposal(proposal.clone()).await?;
        Ok(proposal)
    }

    /// Add a citizen's post to a proposal's deliberation thread ("aye or nay?").
    /// Only a franchised citizen of the proposal's server may speak, and only while
    /// the ballot is still open. An empty body is a no-op.
    pub async fn post_discussion(&self, handle: &str, proposal_id: u64, body: &str) -> Result<(), VoteError> {
        let user = self
            .users
            .find_by_handle(handle.trim()).await?
            .ok_or_else(|| VoteError::NoSuchUser(handle.to_string()))?;
        let mut proposal = self
            .proposals
            .get_proposal(ProposalId(proposal_id)).await?
            .ok_or(VoteError::NoSuchProposal(proposal_id))?;
        if proposal.status != ProposalStatus::Open {
            return Err(VoteError::Closed);
        }
        self.memberships
            .get(user.id, proposal.server_id).await?
            .filter(|m| m.is_franchised(self.clock.now()))
            .ok_or(VoteError::NotACitizen)?;
        let body = body.trim();
        if body.is_empty() {
            return Ok(());
        }
        proposal.discussion.push(DiscussionPost::new(user.id, body, self.clock.now()));
        self.proposals.update_proposal(proposal).await?;
        Ok(())
    }

    /// Read-only: a proposal's deliberation thread, in post order.
    pub async fn list_discussion(&self, proposal_id: u64) -> Vec<DiscussionPost> {
        self.proposals
            .get_proposal(ProposalId(proposal_id)).await.ok().flatten()
            .map(|p| p.discussion)
            .unwrap_or_default()
    }

    /// Read-only: the slug of the server a proposal belongs to (so a driving
    /// adapter can resolve handles/roles against the right server when amending).
    pub async fn proposal_server_slug(&self, proposal_id: u64) -> Option<String> {
        let p = self.proposals.get_proposal(ProposalId(proposal_id)).await.ok().flatten()?;
        self.servers.get_server(p.server_id).await.ok().flatten().map(|s| s.slug)
    }

    /// Resolve every proposal in a server whose window has closed: tally the
    /// weighted votes, decide, and — for a passed, matured ballot — apply the
    /// effect exactly once. Idempotent; safe to call on every read. Returns
    /// `true` when it changed anything (closed a ballot or enacted an effect),
    /// so a caller can persist and notify clients only when state actually moved.
    pub async fn resolve_due(&self, server_slug: &str) -> bool {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() else {
            return false;
        };
        let now = self.clock.now();
        let citizens = self.memberships.citizen_count(server.id).await.unwrap_or_default();
        let phase = Phase::from_citizen_count(citizens, self.phase_thresholds);

        let mut did_change = false;
        for mut p in self.proposals.list_for_server(server.id).await.unwrap_or_default() {
            // Close a ballot whose voting window has elapsed.
            if p.status == ProposalStatus::Open && now >= p.closes_at {
                let tally = self.weighted_tally(&server, &p, now);
                p.close(tally.await, citizens, phase, now);
                self.proposals.update_proposal(p.clone()).await.unwrap_or_default();
                did_change = true;
            }
            // Apply a passed, matured (past any timelock), not-yet-applied effect.
            if let ProposalStatus::Passed { effective_at } = p.status {
                if !p.is_applied && now >= effective_at {
                    self.apply_effect(&p, now).await;
                    p.is_applied = true;
                    self.proposals.update_proposal(p).await.unwrap_or_default();
                    did_change = true;
                }
            }
        }
        did_change
    }

    /// Sum ayes and nays in units of vote weight, honouring the server's
    /// [`domain::VoteWeighting`] and whether it applies to ballots.
    async fn weighted_tally(&self, server: &Server, proposal: &Proposal, now: Timestamp) -> Tally {
        let weighted = server.weighting_scope.applies_to_ballots();
        let mut tally = Tally::default();
        for v in self.votes.list_for_proposal(proposal.id).await.unwrap_or_default() {
            let weight = match self.memberships.get(v.voter, server.id).await.ok().flatten() {
                // A voter who has since been sanctioned is dropped from the count.
                Some(m) if m.is_franchised(now) => {
                    if weighted {
                        server.vote_weighting.weight_of(&m, now)
                    } else {
                        1
                    }
                }
                _ => continue,
            };
            if v.is_aye {
                tally.aye += weight;
            } else {
                tally.nay += weight;
            }
        }
        tally
    }

    /// Apply a passed proposal's effect to the server's stores. A bundle applies
    /// as a unit — the primary change first, then each amendment in the order it
    /// was folded in — so an amended proposal enacts every one of its changes
    /// together (or, having failed, none at all).
    async fn apply_effect(&self, proposal: &Proposal, now: Timestamp) {
        let sid = proposal.server_id;
        for kind in proposal.changes() {
            self.apply_kind(kind, sid, now).await;
        }
    }

    /// Apply a single change to the server's stores. Each arm is the concrete
    /// meaning of a ballot kind.
    async fn apply_kind(&self, kind: &ProposalKind, sid: ServerId, now: Timestamp) {
        match kind {
            ProposalKind::CreateChannel { name, topic, is_voice } => {
                let name = domain::normalize_channel_name(name);
                if !name.is_empty() && self.channels.find_by_name(sid, &name).await.ok().flatten().is_none() {
                    let id = self.channels.next_channel_id().await.unwrap_or_default();
                    let channel = if *is_voice {
                        Channel::voice(id, sid, name, topic.clone(), now)
                    } else {
                        Channel::new(id, sid, name, topic.clone(), now)
                    };
                    self.channels.insert_channel(channel).await.unwrap_or_default();
                }
            }
            ProposalKind::DeleteChannel { name } => {
                let name = domain::normalize_channel_name(name);
                if let Some(c) = self.channels.find_by_name(sid, &name).await.ok().flatten() {
                    self.channels.remove_channel(c.id).await.unwrap_or_default();
                }
            }
            ProposalKind::AddRule { text } => {
                self.rules
                    .insert_rule(Rule::new(self.rules.next_rule_id().await.unwrap_or_default(), sid, text.clone(), now)).await.unwrap_or_default();
            }
            ProposalKind::RemoveRule { rule } => {
                self.rules.remove_rule(*rule).await.unwrap_or_default();
            }
            ProposalKind::Ban { user } => {
                // A ban sanctions the member: it strips the franchise and (via the
                // posting gate) silences them, without deleting their history.
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.is_sanctioned = true;
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::Timeout { user, .. } => {
                // Modelled as a sanction for now (duration handling is future work).
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.is_sanctioned = true;
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::Mute { user } => {
                // A vote-imposed mute has no officer of record (`by: None`), so a
                // later lift bars no one.
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.mute(None);
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::LiftMute { user } => {
                // The electorate overrules the mute. If a police officer imposed it,
                // that officer is barred from re-muting this member for 24 hours.
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.unmute(true, now.plus_days(1));
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::AppointPolice { user } => {
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.is_police = true;
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::DismissPolice { user } => {
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.is_police = false;
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::SetGovernanceSurface { enabled } => {
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.set_governance_surface(enabled.clone());
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::AmendCriteria { proposed } => {
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.criteria = proposed.clone();
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::SetJurySizing { sizing } => {
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.jury_sizing = *sizing;
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::SetVoteWeighting { scheme } => {
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.vote_weighting = *scheme;
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::SetWeightingScope { scope } => {
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.weighting_scope = *scope;
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::GrantVoteWeight { user, weight } => {
                // Adjusts an *already-enfranchised* citizen's ballot weight — never
                // a path into the franchise.
                if let Some(mut m) = self.memberships.get(*user, sid).await.ok().flatten() {
                    m.granted_weight = *weight;
                    self.memberships.upsert(m).await.unwrap_or_default();
                }
            }
            ProposalKind::CreateRole { name, criteria } => {
                let name = domain::normalize_role_name(name);
                if !name.is_empty() && self.roles.find_role(sid, &name).await.ok().flatten().is_none() {
                    let id = self.roles.next_role_id().await.unwrap_or_default();
                    self.roles
                        .insert_role(Role::new(id, sid, name, criteria.clone(), now)).await.unwrap_or_default();
                }
            }
            ProposalKind::DeleteRole { role } => {
                // Only delete a role that belongs to this server.
                if self.roles.get_role(*role).await.ok().flatten().is_some_and(|r| r.server_id == sid) {
                    self.roles.remove_role(*role).await.unwrap_or_default();
                }
            }
            ProposalKind::SetRehomingPolicy { is_disabled } => {
                // Record the community's data-sovereignty choice on the server. A
                // federated node syncs this to the control plane (`set_rehoming`)
                // once the registry is wired in (M4+); the domain flag is the
                // authoritative source. See docs/federation.md §6.
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.is_rehoming_disabled = *is_disabled;
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            ProposalKind::SetInvitePolicy { policy } => {
                // The community's decision on who may admit new members by link.
                if let Some(mut s) = self.servers.get_server(sid).await.ok().flatten() {
                    s.invite_policy = *policy;
                    self.servers.update_server(s).await.unwrap_or_default();
                }
            }
            // Recall and RemoveContent have no persistent server-state effect in
            // this milestone (leadership and content-tombstoning are future work).
            ProposalKind::Recall { .. } | ProposalKind::RemoveContent { .. } => {}
        }
    }

    /// Read-only: every rule in a server (for display).
    pub async fn list_rules(&self, server_slug: &str) -> Vec<Rule> {
        match self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() {
            Some(s) => self.rules.list_for_server(s.id).await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Read-only: every custom emoji in a server.
    pub async fn list_emojis(&self, server_slug: &str) -> Vec<Emoji> {
        match self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() {
            Some(s) => self.emojis.list_for_server(s.id).await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Read-only: a server's proposals, oldest id first. Resolves any due ballots
    /// first so the returned statuses are current.
    pub async fn list_proposals(&self, server_slug: &str) -> Vec<Proposal> {
        self.resolve_due(server_slug).await;
        match self.servers.find_by_slug(server_slug.trim()).await.ok().flatten() {
            Some(s) => self.proposals.list_for_server(s.id).await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Read-only: raw aye/nay **head** counts on a proposal (for live display;
    /// the binding tally at close is weight-aware).
    pub async fn proposal_head_counts(&self, proposal_id: u64) -> (u64, u64) {
        let mut aye = 0;
        let mut nay = 0;
        for v in self.votes.list_for_proposal(ProposalId(proposal_id)).await.unwrap_or_default() {
            if v.is_aye {
                aye += 1;
            } else {
                nay += 1;
            }
        }
        (aye, nay)
    }

    /// Read-only: how `handle` voted on a proposal, if at all.
    pub async fn my_vote(&self, proposal_id: u64, handle: &str) -> Option<bool> {
        let user = self.users.find_by_handle(handle.trim()).await.ok().flatten()?;
        self.votes.get_vote(ProposalId(proposal_id), user.id).await.ok().flatten().map(|v| v.is_aye)
    }
}
