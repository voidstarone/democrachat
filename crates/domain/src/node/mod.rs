//! Node identity and the composite-ID scheme that lets a federated network mint
//! globally-unique IDs without a coordinator.
//!
//! Every entity ID in the system is a `u64`. In a single-box deployment those are
//! just `1, 2, 3, …`. To federate — many nodes, each the source of truth for the
//! servers it hosts and the users it homes — two nodes must never mint the same
//! ID. We solve that *without* a coordinator by **partitioning the `u64`**:
//!
//! ```text
//!  63            48 47                                   0
//! ┌────────────────┬──────────────────────────────────────┐
//! │  node (16 bit) │            sequence (48 bit)          │
//! └────────────────┴──────────────────────────────────────┘
//! ```
//!
//! * The high 16 bits name the **origin node** — the node that minted the ID.
//! * The low 48 bits are that node's local, monotonic sequence.
//!
//! So every ID is globally unique *and self-describing*: [`origin_node`] recovers
//! who minted it. **Node 0 is reserved** for the single-box / bootstrap identity,
//! so `compose_id(NodeId(0), n) == n` and an un-federated deployment is byte-for-
//! byte unchanged. 48 bits is ~281 trillion IDs per node; 16 bits is 65 536 nodes.
//!
//! The domain ID newtypes stay plain `u64` — only the *allocation* strategy
//! changes, so nothing in the governance rules is aware federation exists. See
//! `docs/federation.md`.

pub mod compose_id;
pub mod local_sequence;
pub mod max_sequence;
pub mod node_id;
pub mod origin_node;
pub mod sequence_bits;
pub mod sequence_mask;

pub use compose_id::compose_id;
pub use local_sequence::local_sequence;
pub use max_sequence::MAX_SEQUENCE;
pub use node_id::NodeId;
pub use origin_node::origin_node;
pub use sequence_bits::SEQUENCE_BITS;
pub use sequence_mask::SEQUENCE_MASK;
