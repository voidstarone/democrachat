//! Federation core: node identity and the signed change-event envelope (M2).
//!
//! # Threat model
//!
//! Once servers and the social graph replicate across a network, the transport
//! and the peers are **untrusted**. A hostile or compromised node must not be
//! able to:
//!
//! * **forge** an event (invent a vote, a membership, a removal, a block) that a
//!   peer would apply to its replica;
//! * **tamper** with a legitimate event in flight;
//! * **replay** an old event, or replay one scope's event against another (one
//!   server's vote against another server, one user's block against another);
//! * **impersonate** a scope's owner — which is what would let an attacker split
//!   a server's electorate and defeat the anti-takeover design.
//!
//! # The control
//!
//! Every event that leaves a node is **Ed25519-signed** by that node over a
//! canonical encoding of its *signed part* — `(node, epoch, seq, scope, entity,
//! op, payload)`. A consumer verifies the signature against the producer's public
//! key before applying anything. Binding `epoch` and `seq` gives replay and
//! split-brain protection: an event minted under a stale ownership epoch, or
//! below the consumer's cursor, is rejected even if correctly signed. Binding
//! `scope` stops an event being replayed against another server or user. The key
//! that must match is *the rightful owner's* — resolved from the control plane
//! (M3/M4); this crate provides the verification primitive, not the ownership
//! decision.
//!
//! Node keys are **persistent identities**: a node loads its 32-byte seed from a
//! keyfile or secret env var, never generates one per boot (that would change its
//! identity and invalidate everything it ever signed). [`NodeKeypair::generate`]
//! exists only for keygen tooling and tests.
//!
//! `payload` is opaque to this crate: for an E2EE entity (DMs, message bodies) it
//! is ciphertext, signed and transported without any node reading the plaintext
//! (see `docs/federation.md` §5).

pub mod auth_error;
pub mod authorize;
pub mod change_event;
pub mod change_op;
pub mod change_record;
pub mod change_sink;
pub mod change_source;
pub mod classify;
pub mod derived_scope;
pub mod event_scope;
pub mod fed_error;
mod from_hex;
pub mod ingest;
pub mod ingested;
pub mod node_keypair;
pub mod node_public_key;
pub mod replication_cursor;
pub mod ownership;
pub mod rehome;
pub mod scope_resolver;
pub mod sign_feed;
pub mod signed_part;
mod to_hex;

pub use auth_error::AuthError;
pub use authorize::authorize;
pub use change_event::ChangeEvent;
pub use change_op::ChangeOp;
pub use change_record::ChangeRecord;
pub use change_sink::ChangeSink;
pub use change_source::ChangeSource;
pub use classify::classify;
pub use derived_scope::DerivedScope;
pub use event_scope::EventScope;
pub use fed_error::FedError;
pub use ingest::ingest;
pub use ingested::Ingested;
pub use node_keypair::NodeKeypair;
pub use node_public_key::NodePublicKey;
pub use replication_cursor::ReplicationCursor;
pub use scope_resolver::ScopeResolver;
pub use sign_feed::sign_feed;
pub use signed_part::SignedPart;

pub use ownership::{
    ClaimOutcome, InMemoryRegistry, NodeLoad, NodeStatus, OwnedScope, Ownership, OwnershipRegistry,
    RegistryError,
};
pub use rehome::{choose_new_owner, choose_new_standby, RehomeOutcome, RehomingController};
