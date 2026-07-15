use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use federation::NodeKeypair;

use crate::command::command::Command;
use crate::command::signing_payload::signing_payload;

/// A [`Command`] authenticated to the **forwarding node** by an Ed25519 signature
/// over its canonical bytes.
///
/// The command endpoint is protected by the shared cluster bearer token, but a
/// token is symmetric — anyone holding it could otherwise forward a write naming an
/// arbitrary `voter`. Binding each command to the forwarding node's
/// control-plane-published key means the owner runs a command only from a node it
/// actually knows, and every forwarded write is attributable and non-repudiable. It
/// authenticates the *node*, not the end user (nodes are trusted to have
/// authenticated their own users).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SignedCommand {
    /// The forwarding node id whose key signs the command.
    pub node: u16,
    /// Canonical JSON of the [`Command`].
    pub body: String,
    /// Unix seconds when this command was minted — the owner rejects one outside a
    /// small freshness window so a captured command can't be replayed later.
    pub issued_at: i64,
    /// A per-command unique value; the owner records recently-seen `(node, nonce)`
    /// and refuses a repeat, so the same command can't re-apply within the window.
    pub nonce: String,
    /// Hex Ed25519 signature over `signing_payload(node, issued_at, nonce, body)`,
    /// so none of those fields can be altered after signing.
    pub signature: String,
}

/// A process-unique, non-repeating nonce. It need not be cryptographically random —
/// only unique per command so the owner's replay cache can dedup it — so a monotonic
/// counter mixed with the process's current nanos suffices without an RNG dep.
fn fresh_nonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{base:x}-{n:x}")
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl SignedCommand {
    /// Sign `cmd` with the forwarding node's keypair, stamping a fresh timestamp and
    /// nonce for anti-replay.
    pub fn sign(keypair: &NodeKeypair, cmd: &Command) -> Self {
        Self::sign_at(keypair, cmd, unix_now(), fresh_nonce())
    }

    /// Like [`sign`](Self::sign) but with an explicit timestamp and nonce — used by
    /// tests to exercise freshness and replay deterministically.
    pub fn sign_at(keypair: &NodeKeypair, cmd: &Command, issued_at: i64, nonce: String) -> Self {
        let body = serde_json::to_string(cmd).expect("Command serializes");
        let payload = signing_payload(keypair.node().0, issued_at, &nonce, &body);
        let signature = keypair.sign_hex(payload.as_bytes());
        Self { node: keypair.node().0, body, issued_at, nonce, signature }
    }

    /// The inner command, parsed from the signed bytes. Callers must verify the
    /// signature (see [`verify_signed`](crate::command::verify_signed::verify_signed))
    /// before trusting the result.
    pub fn command(&self) -> Option<Command> {
        serde_json::from_str(&self.body).ok()
    }
}
