//! Voice signaling relay + ephemeral roster.
//!
//! The server is a **blind relay**: it forwards WebRTC SDP offers/answers and ICE
//! candidates between the members of a voice channel's roster, and tracks who is
//! currently present, but never sees the audio itself (media is browser-to-browser,
//! DTLS-SRTP encrypted). The roster is live in-memory state — it is *not* persisted
//! to the store, and is reconstructed from scratch on reconnect.
//!
//! A "connection" is one live WebSocket, identified by a monotonic [`ConnId`]. A
//! single user may hold several (multiple tabs); each is a distinct voice endpoint,
//! so signaling routes to a specific connection, not to a handle.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tokio::sync::mpsc;

/// A monotonic connection id — one per live WebSocket.
pub type ConnId = u64;

/// The full-mesh peer cap. N participants means N² peer connections and O(N) uplink
/// per client, so a mesh room is capped small; larger rooms need an SFU (V6). A join
/// past this cap is refused with a `voice_full` frame.
pub const MAX_ROOM: usize = 8;

/// One live WebSocket's outbound half, plus who owns it.
struct Conn {
    handle: String,
    tx: mpsc::UnboundedSender<String>,
}

/// A voice room's address, `server_slug/channel_name` — unique per voice channel.
fn room_key(server: &str, channel: &str) -> String {
    format!("{server}/{channel}")
}

/// Live signaling state shared across all connections. All fields are behind their
/// own lock; lock order is always `rooms` before `conns`, and no send happens while
/// a lock is held (targets are collected first, then sent).
#[derive(Default)]
pub struct SignalHub {
    next_id: AtomicU64,
    conns: Mutex<HashMap<ConnId, Conn>>,
    /// Room key → the connections currently present in that voice room.
    rooms: Mutex<HashMap<String, HashSet<ConnId>>>,
}

impl SignalHub {
    /// Register a freshly-connected WebSocket. Returns its id and the receiving half
    /// of its outbound queue — the pump forwards everything from this queue to the
    /// socket. Drop the connection with [`unregister`](Self::unregister).
    pub fn register(&self, handle: String) -> (ConnId, mpsc::UnboundedReceiver<String>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        self.conns.lock().unwrap().insert(id, Conn { handle, tx });
        (id, rx)
    }

    /// The distinct handles with at least one live WebSocket right now — the raw
    /// material for the "active users" roster. Deduped (a user open in two tabs
    /// counts once) and unordered; callers scope it to a server themselves.
    pub fn online_handles(&self) -> Vec<String> {
        let conns = self.conns.lock().unwrap();
        let mut seen = HashSet::new();
        conns
            .values()
            .filter(|c| seen.insert(c.handle.clone()))
            .map(|c| c.handle.clone())
            .collect()
    }

    /// Tear a connection down: drop it from the registry and from every room it was
    /// in, telling each room's remaining members it left.
    pub fn unregister(&self, id: ConnId) {
        self.conns.lock().unwrap().remove(&id);
        let mut left_rooms = Vec::new();
        {
            let mut rooms = self.rooms.lock().unwrap();
            rooms.retain(|key, members| {
                if members.remove(&id) {
                    left_rooms.push(key.clone());
                }
                !members.is_empty()
            });
        }
        for key in left_rooms {
            self.announce_leave(&key, id);
        }
    }

    /// Join a voice room. Sends the joiner the current roster (so it can open a peer
    /// connection to each existing member) and tells the existing members a new peer
    /// arrived. Refuses past [`MAX_ROOM`] with a `voice_full` frame.
    pub fn join(&self, id: ConnId, server: &str, channel: &str) {
        let key = room_key(server, channel);
        let peers: Vec<Value>;
        {
            let mut rooms = self.rooms.lock().unwrap();
            let members = rooms.entry(key.clone()).or_default();
            if members.contains(&id) {
                return; // idempotent — already joined (e.g. a resend)
            }
            if members.len() >= MAX_ROOM {
                drop(rooms);
                self.send(id, json!({ "type": "voice_full", "server": server, "channel": channel }));
                return;
            }
            // Snapshot existing members before adding the joiner.
            let conns = self.conns.lock().unwrap();
            peers = members
                .iter()
                .filter_map(|pid| conns.get(pid).map(|c| json!({ "id": pid, "handle": c.handle })))
                .collect();
            members.insert(id);
        }
        let handle = self.handle_of(id);
        // Tell the joiner who is already here (it initiates the offers).
        self.send(
            id,
            json!({ "type": "voice_roster", "server": server, "channel": channel, "you": id, "peers": peers }),
        );
        // Tell everyone else a new peer arrived.
        self.broadcast_room(
            &key,
            id,
            json!({ "type": "voice_peer_join", "server": server, "channel": channel, "id": id, "handle": handle }),
        );
    }

    /// Leave a voice room, telling the remaining members.
    pub fn leave(&self, id: ConnId, server: &str, channel: &str) {
        let key = room_key(server, channel);
        let was_member = {
            let mut rooms = self.rooms.lock().unwrap();
            let Some(members) = rooms.get_mut(&key) else { return };
            let removed = members.remove(&id);
            if members.is_empty() {
                rooms.remove(&key);
            }
            removed
        };
        if was_member {
            self.announce_leave(&key, id);
        }
    }

    /// Relay one signaling payload (an SDP offer/answer or an ICE candidate) from
    /// `from` to `to`. Both must be present in the named room — this bounds the relay
    /// to co-present voice participants, so it cannot be used to message arbitrary
    /// users. The server never inspects `data`; it only stamps the sender.
    pub fn relay(&self, from: ConnId, server: &str, channel: &str, to: ConnId, data: Value) {
        let key = room_key(server, channel);
        {
            let rooms = self.rooms.lock().unwrap();
            let Some(members) = rooms.get(&key) else { return };
            if !members.contains(&from) || !members.contains(&to) {
                return; // sender or target not in this room — drop
            }
        }
        self.send(
            to,
            json!({ "type": "voice_signal", "server": server, "channel": channel, "from": from, "data": data }),
        );
    }

    /// Broadcast a `voice_peer_leave` for `id` to a room's members.
    fn announce_leave(&self, key: &str, id: ConnId) {
        // key is `server/channel`; split once to echo the address back to clients.
        let (server, channel) = key.split_once('/').unwrap_or((key, ""));
        self.broadcast_room(
            key,
            id,
            json!({ "type": "voice_peer_leave", "server": server, "channel": channel, "id": id }),
        );
    }

    /// Send `msg` to every member of a room except `except`.
    fn broadcast_room(&self, key: &str, except: ConnId, msg: Value) {
        let targets: Vec<ConnId> = {
            let rooms = self.rooms.lock().unwrap();
            match rooms.get(key) {
                Some(members) => members.iter().copied().filter(|m| *m != except).collect(),
                None => return,
            }
        };
        let text = msg.to_string();
        let conns = self.conns.lock().unwrap();
        for t in targets {
            if let Some(c) = conns.get(&t) {
                let _ = c.tx.send(text.clone());
            }
        }
    }

    /// Send one JSON frame to a single connection.
    fn send(&self, id: ConnId, msg: Value) {
        if let Some(c) = self.conns.lock().unwrap().get(&id) {
            let _ = c.tx.send(msg.to_string());
        }
    }

    /// The handle owning a connection, or empty if it has gone.
    fn handle_of(&self, id: ConnId) -> String {
        self.conns.lock().unwrap().get(&id).map(|c| c.handle.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;

    /// Drain whatever frames are queued on a receiver right now (non-blocking).
    fn drain(rx: &mut UnboundedReceiver<String>) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(text) = rx.try_recv() {
            out.push(serde_json::from_str(&text).unwrap());
        }
        out
    }

    fn kinds(frames: &[Value]) -> Vec<&str> {
        frames.iter().map(|f| f["type"].as_str().unwrap()).collect()
    }

    #[test]
    fn online_handles_dedupes_and_drops_on_unregister() {
        let hub = SignalHub::default();
        let (a1, _r1) = hub.register("alice".into());
        let (_a2, _r2) = hub.register("alice".into()); // second tab, same user
        let (b, _rb) = hub.register("bob".into());

        let mut online = hub.online_handles();
        online.sort();
        assert_eq!(online, vec!["alice".to_string(), "bob".to_string()], "alice counts once");

        hub.unregister(b);
        assert_eq!(hub.online_handles(), vec!["alice".to_string()], "bob dropped");

        hub.unregister(a1);
        assert_eq!(hub.online_handles(), vec!["alice".to_string()], "alice's other tab keeps her online");
    }

    #[test]
    fn joiner_gets_roster_and_others_get_peer_join() {
        let hub = SignalHub::default();
        let (a, mut a_rx) = hub.register("alice".into());
        let (b, mut b_rx) = hub.register("bob".into());

        hub.join(a, "srv", "lounge");
        // Alice joins an empty room: roster with no peers, nobody else notified.
        let af = drain(&mut a_rx);
        assert_eq!(kinds(&af), ["voice_roster"]);
        assert_eq!(af[0]["you"], a);
        assert_eq!(af[0]["peers"].as_array().unwrap().len(), 0);
        assert!(drain(&mut b_rx).is_empty());

        hub.join(b, "srv", "lounge");
        // Bob's roster now lists Alice...
        let bf = drain(&mut b_rx);
        assert_eq!(kinds(&bf), ["voice_roster"]);
        let peers = bf[0]["peers"].as_array().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0]["id"], a);
        assert_eq!(peers[0]["handle"], "alice");
        // ...and Alice is told a peer joined.
        let af = drain(&mut a_rx);
        assert_eq!(kinds(&af), ["voice_peer_join"]);
        assert_eq!(af[0]["id"], b);
        assert_eq!(af[0]["handle"], "bob");
    }

    #[test]
    fn relay_reaches_only_the_named_target_and_stamps_sender() {
        let hub = SignalHub::default();
        let (a, mut a_rx) = hub.register("alice".into());
        let (b, mut b_rx) = hub.register("bob".into());
        hub.join(a, "srv", "lounge");
        hub.join(b, "srv", "lounge");
        drain(&mut a_rx);
        drain(&mut b_rx);

        hub.relay(a, "srv", "lounge", b, serde_json::json!({ "sdp": "x" }));
        let bf = drain(&mut b_rx);
        assert_eq!(kinds(&bf), ["voice_signal"]);
        assert_eq!(bf[0]["from"], a);
        assert_eq!(bf[0]["data"]["sdp"], "x");
        assert!(drain(&mut a_rx).is_empty()); // sender does not get an echo
    }

    #[test]
    fn relay_to_a_non_member_is_dropped() {
        let hub = SignalHub::default();
        let (a, mut a_rx) = hub.register("alice".into());
        let (b, mut b_rx) = hub.register("bob".into());
        hub.join(a, "srv", "lounge"); // bob never joins
        drain(&mut a_rx);
        hub.relay(a, "srv", "lounge", b, serde_json::json!({ "sdp": "x" }));
        assert!(drain(&mut b_rx).is_empty());
    }

    #[test]
    fn disconnect_announces_leave_to_the_room() {
        let hub = SignalHub::default();
        let (a, mut a_rx) = hub.register("alice".into());
        let (b, mut b_rx) = hub.register("bob".into());
        hub.join(a, "srv", "lounge");
        hub.join(b, "srv", "lounge");
        drain(&mut a_rx);
        drain(&mut b_rx);

        hub.unregister(b);
        let af = drain(&mut a_rx);
        assert_eq!(kinds(&af), ["voice_peer_leave"]);
        assert_eq!(af[0]["id"], b);
        assert_eq!(af[0]["server"], "srv");
        assert_eq!(af[0]["channel"], "lounge");
    }

    #[test]
    fn room_is_capped_at_max() {
        let hub = SignalHub::default();
        let mut held = Vec::new();
        for i in 0..MAX_ROOM {
            let (id, rx) = hub.register(format!("u{i}"));
            hub.join(id, "srv", "lounge");
            held.push((id, rx));
        }
        // One past the cap is refused with voice_full and not added to the room.
        let (over, mut over_rx) = hub.register("over".into());
        hub.join(over, "srv", "lounge");
        let f = drain(&mut over_rx);
        assert_eq!(kinds(&f), ["voice_full"]);
        // The existing members were not told about the rejected joiner.
        let (_, first_rx) = &mut held[0];
        assert!(drain(first_rx).iter().all(|m| m["type"] != "voice_peer_join"
            || m["handle"] != "over"));
    }
}
