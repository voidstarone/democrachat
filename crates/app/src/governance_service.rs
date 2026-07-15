//! Governance use-cases: open a proposal, cast a vote, and — once a ballot's
//! window closes — tally it, decide it, and **apply its effect** to the server.
//!
//! Everything here runs on the pure `domain` rules: the server's governance
//! surface decides *what* may be proposed, `threshold_for`/`decide` decide *how
//! hard* it is to pass, `Proposal::close` applies the phase threshold and the
//! constitutional timelock, and `VoteWeighting` decides how ballots are counted.
//! No effect is applied until a proposal has passed and matured.

use domain::{
    Channel, Emoji, Phase, Proposal, ProposalId, ProposalKind, ProposalStatus, Role,
    RoleAssignment, Rule, Server, Tally, Timestamp, Vote,
};

use crate::{ProposeError, Services, VoteError};

/// How long a ballot accepts votes before it can be closed.
const VOTING_WINDOW_DAYS: i64 = 3;

impl Services {
    /// Open a proposal. Gated on: the proposer is an enfranchised citizen, the
    /// server governs this ballot kind (its surface), and the decision class is
    /// permitted in the server's current phase.
    pub fn open_proposal(
        &self,
        proposer_handle: &str,
        server_slug: &str,
        kind: ProposalKind,
    ) -> Result<Proposal, ProposeError> {
        let user = self
            .users
            .find_by_handle(proposer_handle.trim())
            .ok_or_else(|| ProposeError::NoSuchUser(proposer_handle.to_string()))?;
        let server = self
            .servers
            .find_by_slug(server_slug.trim())
            .ok_or_else(|| ProposeError::NoSuchServer(server_slug.to_string()))?;

        // Only a franchised citizen may propose.
        let membership = self
            .memberships
            .get(user.id, server.id)
            .filter(|m| m.is_franchised())
            .ok_or(ProposeError::NotACitizen)?;
        let _ = membership;

        // Surface check — the server must put this kind of thing to a vote.
        if !server.governs(kind.ballot_kind()) {
            return Err(ProposeError::NotGoverned);
        }

        // Phase check — e.g. constitutional amendments are forbidden in Seed.
        let citizens = self.memberships.citizen_count(server.id);
        let phase = Phase::from_citizen_count(citizens);
        if domain::threshold_for(kind.decision_class(), phase).is_none() {
            return Err(ProposeError::NotAllowedInPhase);
        }

        let now = self.clock.now();
        let proposal = Proposal::new(
            self.proposals.next_proposal_id(),
            server.id,
            user.id,
            kind,
            now,
            now.plus_days(VOTING_WINDOW_DAYS),
        );
        self.proposals.insert_proposal(proposal.clone());
        Ok(proposal)
    }

    /// Cast (or change) a vote on an open proposal. One ballot per citizen.
    pub fn cast_vote(&self, voter_handle: &str, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let user = self
            .users
            .find_by_handle(voter_handle.trim())
            .ok_or_else(|| VoteError::NoSuchUser(voter_handle.to_string()))?;
        self.cast_vote_as(user.id, proposal_id, is_aye)
    }

    /// Cast a vote identified by the voter's **user id** rather than handle — the
    /// path a *forwarded* federation command takes, where the voter is a federated
    /// user the owner knows only by id (their home node authenticated them; the
    /// owner still re-checks citizenship here, never trusting the forwarder).
    pub fn cast_vote_by_id(&self, voter_id: u64, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let user = self
            .users
            .get_user(domain::UserId(voter_id))
            .ok_or_else(|| VoteError::NoSuchUser(voter_id.to_string()))?;
        self.cast_vote_as(user.id, proposal_id, is_aye)
    }

    /// The shared core: validate the proposal is open and the voter is an
    /// enfranchised citizen of its server, then record the ballot. The two public
    /// entry points differ only in how they resolve the voter (handle vs id).
    fn cast_vote_as(&self, voter_id: domain::UserId, proposal_id: u64, is_aye: bool) -> Result<(), VoteError> {
        let proposal = self
            .proposals
            .get_proposal(ProposalId(proposal_id))
            .ok_or(VoteError::NoSuchProposal(proposal_id))?;
        if proposal.status != ProposalStatus::Open {
            return Err(VoteError::Closed);
        }
        // Must be a franchised citizen of this server.
        self.memberships
            .get(voter_id, proposal.server_id)
            .filter(|m| m.is_franchised())
            .ok_or(VoteError::NotACitizen)?;

        self.votes.upsert_vote(Vote::new(proposal.id, voter_id, is_aye));
        Ok(())
    }

    /// Resolve every proposal in a server whose window has closed: tally the
    /// weighted votes, decide, and — for a passed, matured ballot — apply the
    /// effect exactly once. Idempotent; safe to call on every read.
    pub fn resolve_due(&self, server_slug: &str) {
        let Some(server) = self.servers.find_by_slug(server_slug.trim()) else {
            return;
        };
        let now = self.clock.now();
        let citizens = self.memberships.citizen_count(server.id);
        let phase = Phase::from_citizen_count(citizens);

        for mut p in self.proposals.list_for_server(server.id) {
            // Close a ballot whose voting window has elapsed.
            if p.status == ProposalStatus::Open && now >= p.closes_at {
                let tally = self.weighted_tally(&server, &p, now);
                p.close(tally, citizens, phase, now);
                self.proposals.update_proposal(p.clone());
            }
            // Apply a passed, matured (past any timelock), not-yet-applied effect.
            if let ProposalStatus::Passed { effective_at } = p.status {
                if !p.is_applied && now >= effective_at {
                    self.apply_effect(&p, now);
                    p.is_applied = true;
                    self.proposals.update_proposal(p);
                }
            }
        }
    }

    /// Sum ayes and nays in units of vote weight, honouring the server's
    /// [`domain::VoteWeighting`] and whether it applies to ballots.
    fn weighted_tally(&self, server: &Server, proposal: &Proposal, now: Timestamp) -> Tally {
        let weighted = server.weighting_scope.applies_to_ballots();
        let mut tally = Tally::default();
        for v in self.votes.list_for_proposal(proposal.id) {
            let weight = match self.memberships.get(v.voter, server.id) {
                // A voter who has since been sanctioned is dropped from the count.
                Some(m) if m.is_franchised() => {
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

    /// Apply a passed proposal's effect to the server's stores. Each arm is the
    /// concrete meaning of a ballot kind.
    fn apply_effect(&self, proposal: &Proposal, now: Timestamp) {
        let sid = proposal.server_id;
        match &proposal.kind {
            ProposalKind::CreateChannel { name, topic } => {
                let name = domain::normalize_channel_name(name);
                if !name.is_empty() && self.channels.find_by_name(sid, &name).is_none() {
                    self.channels.insert_channel(Channel::new(
                        self.channels.next_channel_id(),
                        sid,
                        name,
                        topic.clone(),
                        now,
                    ));
                }
            }
            ProposalKind::DeleteChannel { name } => {
                let name = domain::normalize_channel_name(name);
                if let Some(c) = self.channels.find_by_name(sid, &name) {
                    self.channels.remove_channel(c.id);
                }
            }
            ProposalKind::AddRule { text } => {
                self.rules
                    .insert_rule(Rule::new(self.rules.next_rule_id(), sid, text.clone(), now));
            }
            ProposalKind::RemoveRule { rule } => {
                self.rules.remove_rule(*rule);
            }
            ProposalKind::Ban { user } => {
                // A ban sanctions the member: it strips the franchise and (via the
                // posting gate) silences them, without deleting their history.
                if let Some(mut m) = self.memberships.get(*user, sid) {
                    m.is_sanctioned = true;
                    self.memberships.upsert(m);
                }
            }
            ProposalKind::Timeout { user, .. } => {
                // Modelled as a sanction for now (duration handling is future work).
                if let Some(mut m) = self.memberships.get(*user, sid) {
                    m.is_sanctioned = true;
                    self.memberships.upsert(m);
                }
            }
            ProposalKind::SetGovernanceSurface { enabled } => {
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.set_governance_surface(enabled.clone());
                    self.servers.update_server(s);
                }
            }
            ProposalKind::AmendCriteria { proposed } => {
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.criteria = proposed.clone();
                    self.servers.update_server(s);
                }
            }
            ProposalKind::SetJurySizing { sizing } => {
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.jury_sizing = *sizing;
                    self.servers.update_server(s);
                }
            }
            ProposalKind::SetVoteWeighting { scheme } => {
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.vote_weighting = *scheme;
                    self.servers.update_server(s);
                }
            }
            ProposalKind::SetWeightingScope { scope } => {
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.weighting_scope = *scope;
                    self.servers.update_server(s);
                }
            }
            ProposalKind::GrantVoteWeight { user, weight } => {
                // Adjusts an *already-enfranchised* citizen's ballot weight — never
                // a path into the franchise.
                if let Some(mut m) = self.memberships.get(*user, sid) {
                    m.granted_weight = *weight;
                    self.memberships.upsert(m);
                }
            }
            ProposalKind::CreateRole { name } => {
                let name = domain::normalize_role_name(name);
                if !name.is_empty() && self.roles.find_role(sid, &name).is_none() {
                    self.roles
                        .insert_role(Role::new(self.roles.next_role_id(), sid, name, now));
                }
            }
            ProposalKind::DeleteRole { role } => {
                // Only delete a role that belongs to this server.
                if self.roles.get_role(*role).is_some_and(|r| r.server_id == sid) {
                    self.roles.remove_role(*role);
                }
            }
            ProposalKind::AssignRole { user, role } => {
                if self.roles.get_role(*role).is_some_and(|r| r.server_id == sid) {
                    self.roles.assign(RoleAssignment::new(sid, *role, *user));
                }
            }
            ProposalKind::UnassignRole { user, role } => {
                if self.roles.get_role(*role).is_some_and(|r| r.server_id == sid) {
                    self.roles.unassign(*role, *user);
                }
            }
            ProposalKind::SetRehomingPolicy { is_disabled } => {
                // Record the community's data-sovereignty choice on the server. A
                // federated node syncs this to the control plane (`set_rehoming`)
                // once the registry is wired in (M4+); the domain flag is the
                // authoritative source. See docs/federation.md §6.
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.is_rehoming_disabled = *is_disabled;
                    self.servers.update_server(s);
                }
            }
            ProposalKind::SetInvitePolicy { policy } => {
                // The community's decision on who may admit new members by link.
                if let Some(mut s) = self.servers.get_server(sid) {
                    s.invite_policy = *policy;
                    self.servers.update_server(s);
                }
            }
            // Recall and RemoveContent have no persistent server-state effect in
            // this milestone (leadership and content-tombstoning are future work).
            ProposalKind::Recall { .. } | ProposalKind::RemoveContent { .. } => {}
        }
    }

    /// Read-only: every rule in a server (for display).
    pub fn list_rules(&self, server_slug: &str) -> Vec<Rule> {
        match self.servers.find_by_slug(server_slug.trim()) {
            Some(s) => self.rules.list_for_server(s.id),
            None => Vec::new(),
        }
    }

    /// Read-only: every custom emoji in a server.
    pub fn list_emojis(&self, server_slug: &str) -> Vec<Emoji> {
        match self.servers.find_by_slug(server_slug.trim()) {
            Some(s) => self.emojis.list_for_server(s.id),
            None => Vec::new(),
        }
    }

    /// Read-only: a server's proposals, oldest id first. Resolves any due ballots
    /// first so the returned statuses are current.
    pub fn list_proposals(&self, server_slug: &str) -> Vec<Proposal> {
        self.resolve_due(server_slug);
        match self.servers.find_by_slug(server_slug.trim()) {
            Some(s) => self.proposals.list_for_server(s.id),
            None => Vec::new(),
        }
    }

    /// Read-only: raw aye/nay **head** counts on a proposal (for live display;
    /// the binding tally at close is weight-aware).
    pub fn proposal_head_counts(&self, proposal_id: u64) -> (u64, u64) {
        let mut aye = 0;
        let mut nay = 0;
        for v in self.votes.list_for_proposal(ProposalId(proposal_id)) {
            if v.is_aye {
                aye += 1;
            } else {
                nay += 1;
            }
        }
        (aye, nay)
    }

    /// Read-only: how `handle` voted on a proposal, if at all.
    pub fn my_vote(&self, proposal_id: u64, handle: &str) -> Option<bool> {
        let user = self.users.find_by_handle(handle.trim())?;
        self.votes.get_vote(ProposalId(proposal_id), user.id).map(|v| v.is_aye)
    }
}
