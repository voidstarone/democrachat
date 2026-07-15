//! Cross-browser voice-mesh end-to-end test. Boots the real server and drives one
//! **Chrome** and one **Firefox** into the same voice channel, asserting the
//! full-mesh WebRTC connection reaches `connected` with a live remote audio track
//! each way (see `e2e/voice-mesh.mjs`).
//!
//! This exercises the whole stack the unit/integration tests can't: the browser
//! `RTCPeerConnection` mesh, the WebSocket signaling relay, and the security headers
//! (a `Permissions-Policy` that denied the microphone would fail it). It is heavy —
//! it needs Node, `puppeteer-core`, and both browsers installed — so it runs **only**
//! when `DEMOCRACHAT_E2E=1`. Without that flag it is a no-op, keeping `cargo test`
//! green on machines (and CI) without a desktop browser.
//!
//! Run it with:  `DEMOCRACHAT_E2E=1 cargo test -p democrachat --test voice_e2e`

use std::path::PathBuf;
use std::process::Command;

/// Repository root (two levels up from this crate's manifest dir).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn chrome_and_firefox_share_a_voice_mesh() {
    if std::env::var("DEMOCRACHAT_E2E").ok().as_deref() != Some("1") {
        eprintln!("skipping: set DEMOCRACHAT_E2E=1 to run the browser voice-mesh e2e");
        return;
    }
    let root = repo_root();
    let e2e = root.join("e2e");

    // Restore the harness's Node dependencies if they aren't present yet.
    if !e2e.join("node_modules").is_dir() {
        let install = Command::new("npm")
            .args(["install", "--no-audit", "--no-fund"])
            .current_dir(&e2e)
            .status()
            .expect("run `npm install` in e2e/");
        assert!(install.success(), "npm install failed");
    }

    // `cargo test -p democrachat` builds the binary target first, so it exists here.
    let status = Command::new("node")
        .arg(e2e.join("voice-mesh.mjs"))
        .current_dir(&root)
        .status()
        .expect("run the node voice-mesh harness");
    assert!(status.success(), "cross-browser voice-mesh e2e failed (see output above)");
}
