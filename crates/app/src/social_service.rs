//! Social use-cases: direct messages, permanent blocks, and friendships.
//!
//! DMs are platform-wide — they belong to no server — and are **on by default**.
//! Whether one user may message another is decided by the single pure predicate
//! [`domain::can_dm`], fed the two users' shared blocks and friendships plus the
//! recipient's [`DmPolicy`](domain::DmPolicy). A [`Block`](domain::Block) is
//! permanent and silences DMs both ways; a [`Friendship`](domain::Friendship)
//! backs the friends-only policy. No governance touches this layer — it is
//! account-to-account, not server business.

use domain::{Block, DmMessage, DmPolicy, Friendship, User, UserId};

use crate::{DmError, Services, SocialError};

impl Services {
    /// Send an **end-to-end-encrypted** direct message from `from_handle` to
    /// `to_handle`. The client has already sealed the body against both device keys
    /// (recipient's and sender's, fetched from the key directory) — the server only
    /// stores the two ciphertexts and can never read them. Gated by
    /// [`domain::can_dm`]: a block in either direction, or a friends-only recipient
    /// the sender isn't friends with, yields [`DmError::NotAllowed`]. The gate reads
    /// only metadata (blocks, friendships, policy), never the body.
    pub fn send_sealed_dm(
        &self,
        from_handle: &str,
        to_handle: &str,
        sealed_for_recipient: &str,
        sealed_for_sender: &str,
    ) -> Result<DmMessage, DmError> {
        let sender = self
            .users
            .find_by_handle(from_handle.trim())
            .ok_or_else(|| DmError::NoSuchUser(from_handle.to_string()))?;
        let recipient = self
            .users
            .find_by_handle(to_handle.trim())
            .ok_or_else(|| DmError::NoSuchUser(to_handle.to_string()))?;
        self.send_sealed_dm_core(sender, recipient, sealed_for_recipient, sealed_for_sender)
    }

    /// The by-**id** form used by the federation command executor: when a DM write
    /// is forwarded to the sender's home node, the owner runs it here, re-checking
    /// the `can_dm` gate itself (it never trusts the forwarder). Shares the core
    /// with the handle-based [`send_sealed_dm`](Self::send_sealed_dm).
    pub fn send_sealed_dm_by_id(
        &self,
        from_id: u64,
        to_id: u64,
        sealed_for_recipient: &str,
        sealed_for_sender: &str,
    ) -> Result<DmMessage, DmError> {
        let sender = self
            .users
            .get_user(UserId(from_id))
            .ok_or_else(|| DmError::NoSuchUser(from_id.to_string()))?;
        let recipient = self
            .users
            .get_user(UserId(to_id))
            .ok_or_else(|| DmError::NoSuchUser(to_id.to_string()))?;
        self.send_sealed_dm_core(sender, recipient, sealed_for_recipient, sealed_for_sender)
    }

    /// Shared core: gate on `can_dm` (reading only metadata) and store the two
    /// ciphertexts. The server never sees the body.
    fn send_sealed_dm_core(
        &self,
        sender: User,
        recipient: User,
        sealed_for_recipient: &str,
        sealed_for_sender: &str,
    ) -> Result<DmMessage, DmError> {
        if sender.id == recipient.id {
            return Err(DmError::Self_);
        }
        // The server is blind to the body; it can only insist a ciphertext is
        // actually present for each party.
        if sealed_for_recipient.trim().is_empty() || sealed_for_sender.trim().is_empty() {
            return Err(DmError::EmptyBody);
        }
        if !self.may_dm(&sender, &recipient) {
            return Err(DmError::NotAllowed);
        }

        let message = DmMessage::new(
            self.dms.next_dm_id(),
            sender.id,
            recipient.id,
            sealed_for_recipient.trim(),
            sealed_for_sender.trim(),
            self.clock.now(),
        );
        self.dms.insert_dm(message.clone());
        Ok(message)
    }

    /// Whether `sender` may currently DM `recipient`, per the domain rule. Reads
    /// only the blocks and friendships that involve the pair.
    fn may_dm(&self, sender: &User, recipient: &User) -> bool {
        let blocks = self.blocks.involving(recipient.id);
        let friendships = self.friends.involving(recipient.id);
        domain::can_dm(sender.id, recipient.id, recipient.dm_policy, &blocks, &friendships)
    }

    /// Read-only: whether `from_handle` may DM `to_handle` right now — powers a
    /// disabled compose box without attempting a send.
    pub fn can_dm(&self, from_handle: &str, to_handle: &str) -> bool {
        let (Some(sender), Some(recipient)) = (
            self.users.find_by_handle(from_handle.trim()),
            self.users.find_by_handle(to_handle.trim()),
        ) else {
            return false;
        };
        sender.id != recipient.id && self.may_dm(&sender, &recipient)
    }

    /// Read-only: the conversation between two users, oldest first.
    pub fn conversation(&self, a_handle: &str, b_handle: &str) -> Vec<DmMessage> {
        let (Some(a), Some(b)) = (
            self.users.find_by_handle(a_handle.trim()),
            self.users.find_by_handle(b_handle.trim()),
        ) else {
            return Vec::new();
        };
        self.dms.conversation(a.id, b.id)
    }

    /// Read-only: the handles a user has DM conversations with, most-recent first.
    pub fn dm_partners(&self, handle: &str) -> Vec<String> {
        let Some(user) = self.users.find_by_handle(handle.trim()) else {
            return Vec::new();
        };
        self.dms
            .partners(user.id)
            .into_iter()
            .filter_map(|id| self.users.get_user(id))
            .map(|u| u.handle)
            .collect()
    }

    /// Set a user's DM policy (everyone vs. friends-only). This is the "disable
    /// DMs for non-friends" control.
    pub fn set_dm_policy(&self, handle: &str, policy: DmPolicy) -> Result<(), SocialError> {
        let mut user = self
            .users
            .find_by_handle(handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(handle.to_string()))?;
        user.dm_policy = policy;
        self.users.update_user(user);
        Ok(())
    }

    /// Read-only: a user's current DM policy.
    pub fn dm_policy(&self, handle: &str) -> Option<DmPolicy> {
        self.users.find_by_handle(handle.trim()).map(|u| u.dm_policy)
    }

    /// Permanently block another user. Idempotent — blocking again is a no-op. A
    /// block silences DMs in both directions and is never lifted.
    pub fn block_user(&self, blocker_handle: &str, blocked_handle: &str) -> Result<(), SocialError> {
        let blocker = self
            .users
            .find_by_handle(blocker_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(blocker_handle.to_string()))?;
        let blocked = self
            .users
            .find_by_handle(blocked_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(blocked_handle.to_string()))?;
        self.block_between(blocker, blocked)
    }

    /// Block by home-stable user id. This is the path a forwarded `Command::Block`
    /// runs on each of the two users' homes: the owner authorizes by id, having never
    /// seen the caller's handles. Because a block is committed on *both* homes, this
    /// is called once per home with the same pair — idempotent, so the two commits
    /// (and any re-drive of a partial one) converge on the same record.
    pub fn block_user_by_id(&self, blocker_id: u64, blocked_id: u64) -> Result<(), SocialError> {
        let blocker = self
            .users
            .get_user(UserId(blocker_id))
            .ok_or_else(|| SocialError::NoSuchUser(blocker_id.to_string()))?;
        let blocked = self
            .users
            .get_user(UserId(blocked_id))
            .ok_or_else(|| SocialError::NoSuchUser(blocked_id.to_string()))?;
        self.block_between(blocker, blocked)
    }

    /// Record a permanent block between two resolved users. Idempotent; refuses a
    /// self-block.
    fn block_between(&self, blocker: User, blocked: User) -> Result<(), SocialError> {
        if blocker.id == blocked.id {
            return Err(SocialError::Self_);
        }
        self.blocks
            .add(Block::new(blocker.id, blocked.id, self.clock.now()));
        Ok(())
    }

    /// Read-only: the users *this* user has blocked (blocks they initiated), by
    /// handle. Powers the client's "Blocked" list. A block is permanent, so this is
    /// display-only — there is deliberately no unblock.
    pub fn blocked_users(&self, handle: &str) -> Vec<String> {
        let Some(user) = self.users.find_by_handle(handle.trim()) else {
            return Vec::new();
        };
        self.blocks
            .involving(user.id)
            .into_iter()
            .filter(|b| b.blocker == user.id)
            .filter_map(|b| self.users.get_user(b.blocked))
            .map(|u| u.handle)
            .collect()
    }

    /// Read-only: whether `a_handle` and `b_handle` have a block between them.
    pub fn is_blocked_between(&self, a_handle: &str, b_handle: &str) -> bool {
        let (Some(a), Some(b)) = (
            self.users.find_by_handle(a_handle.trim()),
            self.users.find_by_handle(b_handle.trim()),
        ) else {
            return false;
        };
        self.blocks.is_blocked_between(a.id, b.id)
    }

    /// Send (or re-affirm) a friend request. Idempotent: if a record already exists
    /// between the two — pending or accepted — this is a no-op. A request *toward*
    /// someone who already requested you does not auto-accept; use [`accept_friend`].
    pub fn request_friend(
        &self,
        requester_handle: &str,
        addressee_handle: &str,
    ) -> Result<(), SocialError> {
        let requester = self
            .users
            .find_by_handle(requester_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(requester_handle.to_string()))?;
        let addressee = self
            .users
            .find_by_handle(addressee_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(addressee_handle.to_string()))?;
        self.request_friend_between(requester, addressee)
    }

    /// Request by home-stable id — the path a forwarded `Command::RequestFriend` runs
    /// on *both* users' homes. Like a block, a friendship is a two-user record: the
    /// addressee's home needs it to show the incoming request, the requester's to show
    /// the outgoing one, and once accepted the friends-only DM gate (which runs on the
    /// *sender's* home) reads it there — so it must land on both. Idempotent.
    pub fn request_friend_by_id(
        &self,
        requester_id: u64,
        addressee_id: u64,
    ) -> Result<(), SocialError> {
        let requester = self
            .users
            .get_user(UserId(requester_id))
            .ok_or_else(|| SocialError::NoSuchUser(requester_id.to_string()))?;
        let addressee = self
            .users
            .get_user(UserId(addressee_id))
            .ok_or_else(|| SocialError::NoSuchUser(addressee_id.to_string()))?;
        self.request_friend_between(requester, addressee)
    }

    /// Record (or re-affirm) a pending request between two resolved users.
    fn request_friend_between(
        &self,
        requester: User,
        addressee: User,
    ) -> Result<(), SocialError> {
        if requester.id == addressee.id {
            return Err(SocialError::Self_);
        }
        self.friends
            .add(Friendship::request(requester.id, addressee.id, self.clock.now()));
        Ok(())
    }

    /// Accept a pending friend request that `other_handle` sent to `me_handle`.
    pub fn accept_friend(&self, me_handle: &str, other_handle: &str) -> Result<(), SocialError> {
        let me = self
            .users
            .find_by_handle(me_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(me_handle.to_string()))?;
        let other = self
            .users
            .find_by_handle(other_handle.trim())
            .ok_or_else(|| SocialError::NoSuchUser(other_handle.to_string()))?;
        self.accept_friend_between(me, other)
    }

    /// Accept by home-stable id — the path a forwarded `Command::AcceptFriend` runs on
    /// both users' homes. The pending request is already on both (from the request's
    /// own 2-home commit), so each home flips its copy to accepted and they converge.
    pub fn accept_friend_by_id(
        &self,
        accepter_id: u64,
        requester_id: u64,
    ) -> Result<(), SocialError> {
        let me = self
            .users
            .get_user(UserId(accepter_id))
            .ok_or_else(|| SocialError::NoSuchUser(accepter_id.to_string()))?;
        let other = self
            .users
            .get_user(UserId(requester_id))
            .ok_or_else(|| SocialError::NoSuchUser(requester_id.to_string()))?;
        self.accept_friend_between(me, other)
    }

    /// Flip the pending request `other`→`me` to accepted. Only the addressee of a
    /// *pending* request may accept it.
    fn accept_friend_between(&self, me: User, other: User) -> Result<(), SocialError> {
        let mut friendship = self
            .friends
            .between(me.id, other.id)
            .filter(|f| !f.are_friends() && f.requester == other.id && f.addressee == me.id)
            .ok_or(SocialError::NoPendingRequest)?;
        friendship.accept();
        self.friends.update(friendship);
        Ok(())
    }

    /// Read-only: whether two users are accepted, mutual friends.
    pub fn are_friends(&self, a_handle: &str, b_handle: &str) -> bool {
        let (Some(a), Some(b)) = (
            self.users.find_by_handle(a_handle.trim()),
            self.users.find_by_handle(b_handle.trim()),
        ) else {
            return false;
        };
        self.friends
            .between(a.id, b.id)
            .is_some_and(|f| f.are_friends())
    }

    /// Read-only: the accepted friends of a user, by handle.
    pub fn friends_of(&self, handle: &str) -> Vec<String> {
        let Some(user) = self.users.find_by_handle(handle.trim()) else {
            return Vec::new();
        };
        self.friends
            .involving(user.id)
            .into_iter()
            .filter(|f| f.are_friends())
            .filter_map(|f| self.other_party(&f, user.id))
            .filter_map(|id| self.users.get_user(id))
            .map(|u| u.handle)
            .collect()
    }

    /// Read-only: pending friend requests *awaiting this user's* answer, by the
    /// requester's handle.
    pub fn incoming_friend_requests(&self, handle: &str) -> Vec<String> {
        let Some(user) = self.users.find_by_handle(handle.trim()) else {
            return Vec::new();
        };
        self.friends
            .involving(user.id)
            .into_iter()
            .filter(|f| !f.are_friends() && f.addressee == user.id)
            .filter_map(|f| self.users.get_user(f.requester))
            .map(|u| u.handle)
            .collect()
    }

    /// The party of a friendship that isn't `me`.
    fn other_party(&self, friendship: &Friendship, me: UserId) -> Option<UserId> {
        if friendship.requester == me {
            Some(friendship.addressee)
        } else if friendship.addressee == me {
            Some(friendship.requester)
        } else {
            None
        }
    }
}
