//! democrachat composition root — the only crate that names concrete adapters.
//!
//! It wires an in-memory store (persisted to a JSON file between runs) and a
//! controllable clock into the `app` use-cases, then hands them to a driving
//! adapter: the CLI for one-shot commands, or the web server (`serve`) for the
//! interactive realtime demo. Swapping storage or delivery is a change *here* and
//! nowhere else — `domain` and `app` never learn what was chosen.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore, SystemClock};
use app::Clock;
use app::Services;
use app::SessionSigner;

mod block_router;
mod command_executor;
mod dm_router;
mod federation;
mod friend_router;
mod nonce_log;
mod pg_nonce_log;
mod vote_router;

/// The media storage directory (a separate concern from the shard snapshot).
fn media_dir() -> PathBuf {
    std::env::var_os("DEMOCRACHAT_MEDIA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("media"))
}

fn data_path() -> PathBuf {
    std::env::var_os("DEMOCRACHAT_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("democrachat.json"))
}

/// This deployment's node identity, from `DEMOCRACHAT_NODE_ID`. Unset ⇒
/// `NodeId(0)`, the single-box / bootstrap identity, so an un-federated
/// deployment mints the same `1, 2, 3…` IDs as before. A federated node sets a
/// distinct 16-bit id (1–65535); a value that doesn't parse is a hard error
/// rather than a silent fall-back to node 0 (which would risk ID collisions).
fn node_id_from_env() -> domain::NodeId {
    match std::env::var("DEMOCRACHAT_NODE_ID") {
        Err(_) => domain::NodeId(0),
        Ok(raw) => match raw.trim().parse::<u16>() {
            Ok(id) => domain::NodeId(id),
            Err(e) => {
                eprintln!("error: DEMOCRACHAT_NODE_ID must be 0–65535: {e}");
                exit(2);
            }
        },
    }
}

/// This node's at-rest data key, from `DEMOCRACHAT_DATA_KEK` (64 hex chars).
/// Unset ⇒ `None`: the store persists as plaintext (the single-box dev default,
/// unchanged). Set-but-invalid is a hard error rather than a silent plaintext
/// fall-back, which would defeat the point of configuring encryption at all.
fn data_key_from_env() -> Option<app::VaultKey> {
    let raw = std::env::var("DEMOCRACHAT_DATA_KEK").ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    match app::VaultKey::from_hex(&raw) {
        Ok(key) => Some(key),
        Err(e) => {
            eprintln!("error: DEMOCRACHAT_DATA_KEK is invalid: {e}");
            exit(2);
        }
    }
}

/// Read the persisted dataset as plaintext JSON, transparently unsealing it when a
/// data key is configured. Returns `None` if there is no file yet.
///
/// Migration is one-directional and safe: an existing plaintext file is loaded as
/// is and gets sealed on the next save; a sealed file cannot be read without the
/// key (a hard error, so a misconfigured node never silently serves an empty
/// dataset over real, encrypted data).
fn load_dataset(path: &PathBuf, key: Option<&app::VaultKey>) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    match (key, app::Sealed::from_json(&raw)) {
        // Configured + sealed file: decrypt.
        (Some(key), Ok(sealed)) => {
            let bytes = app::vault_open(key, &sealed).unwrap_or_else(|e| {
                eprintln!("error: could not decrypt {}: {e}", path.display());
                exit(2);
            });
            Some(String::from_utf8(bytes).unwrap_or_else(|e| {
                eprintln!("error: decrypted {} is not valid UTF-8: {e}", path.display());
                exit(2);
            }))
        }
        // Configured but the file is still plaintext: load it (migrates on save).
        (Some(_), Err(_)) => Some(raw),
        // No key but the file is sealed: refuse rather than lose the data.
        (None, Ok(_)) => {
            eprintln!(
                "error: {} is encrypted but DEMOCRACHAT_DATA_KEK is not set",
                path.display()
            );
            exit(2);
        }
        // No key, plaintext file: the unchanged default.
        (None, Err(_)) => Some(raw),
    }
}

fn save(store: &MemoryStore, path: &PathBuf, key: Option<&app::VaultKey>) {
    let Ok(json) = store.to_json() else {
        return;
    };
    // Seal at rest when a key is configured; otherwise write plaintext (unchanged).
    let out = match key {
        Some(key) => app::vault_seal(key, json.as_bytes()).to_json(),
        None => json,
    };
    if let Err(e) = std::fs::write(path, out) {
        eprintln!("warning: could not save to {}: {e}", path.display());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = data_path();

    // Production persistence: when `DATABASE_URL` is set, serve against Postgres
    // (the source of truth) instead of the in-memory file store. Postgres backs the
    // web app only — the file store + JSON snapshot stay the dev/CLI backend, and
    // federation (which replicates the in-memory outbox) is not yet wired onto PG.
    if let Ok(database_url) = std::env::var("DATABASE_URL") {
        serve_postgres(&args, database_url);
        return;
    }

    let node = node_id_from_env();
    // The at-rest key (if any) is needed to both load and save the dataset.
    let data_key = data_key_from_env();
    let store = Arc::new(
        match load_dataset(&path, data_key.as_ref()) {
            Some(json) => MemoryStore::from_json(&json).unwrap_or_else(|e| {
                eprintln!("error: {} is corrupt: {e}", path.display());
                exit(2);
            }),
            None => MemoryStore::new(),
        }
        // The node identity is a deployment property (env/config), never data —
        // it stamps every newly-minted ID so a federated network stays
        // collision-free. Unset ⇒ NodeId(0), the single-box identity (IDs
        // numerically unchanged). See docs/federation.md.
        .with_node(node),
    );

    // The clock starts at wall-clock time; the CLI's `--now` and the web `--dev`
    // fast-forward both drive this same controllable clock.
    let clock = Arc::new(FixedClock::new(SystemClock.now()));

    // One in-memory store backs every persistence port — except media, which is a
    // separate storage tier: blobs live as files under the media directory
    // (`DEMOCRACHAT_MEDIA_DIR`, default `./media`), never in the JSON snapshot or
    // the replication feed.
    let mut stores = store.as_stores();
    match adapter_media_fs::FsMediaStore::new(media_dir()) {
        Ok(media) => stores.media = Arc::new(media),
        Err(e) => {
            eprintln!("error: cannot open media directory {}: {e}", media_dir().display());
            exit(2);
        }
    }
    // Re-encode uploaded images (strip metadata/exploits, HEIC/HEIF → JPEG). Only
    // production wires the real codec; tests keep the identity transcoder.
    stores.image = Arc::new(adapter_image::ReencodingTranscoder::new());
    let services = Arc::new(Services::new(clock.clone(), stores));

    // Backfill floor channels for any server persisted before channels were
    // auto-provisioned (older datasets have servers with no channels, and none had
    // an #appeals room). Idempotent; the next save writes the repaired dataset.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("backfill runtime")
        .block_on(services.backfill_default_channels());

    if args.get(1).map(String::as_str) == Some("serve") {
        run_serve(&args, services, clock, store, path, node, data_key);
        return;
    }

    let clock_for_setter = clock.clone();
    let code = adapter_cli::run(&services, move |ts| clock_for_setter.set(ts));
    save(&store, &path, data_key.as_ref());
    exit(code);
}

fn run_serve(
    args: &[String],
    services: Arc<Services>,
    clock: Arc<FixedClock>,
    store: Arc<MemoryStore>,
    path: PathBuf,
    node: domain::NodeId,
    data_key: Option<app::VaultKey>,
) {
    let is_dev = args.iter().any(|a| a == "--dev");
    let addr: SocketAddr = arg_value(args, "--addr")
        .unwrap_or_else(|| "127.0.0.1:3737".to_string())
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("error: bad --addr: {e}");
            exit(2);
        });

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(seed_if_empty(&services));
    save(&store, &path, data_key.as_ref());

    // Wire the dev clock fast-forward to the same FixedClock.
    let advance: Arc<dyn Fn(i64) + Send + Sync> = {
        let c = clock.clone();
        Arc::new(move |days| {
            let now = c.now();
            c.set(now.plus_days(days));
        })
    };
    // Persist after each mutation, matching the CLI's save-on-exit behaviour.
    let save_hook: Arc<dyn Fn() + Send + Sync> = {
        let s = store.clone();
        let p = path.clone();
        let k = data_key.clone();
        Arc::new(move || save(&s, &p, k.as_ref()))
    };

    // Session-signing key + cookie policy come from the environment, fail-closed
    // on a public bind (see `session_signer_from_env`).
    let is_loopback = addr.ip().is_loopback();
    let signer = Arc::new(session_signer_from_env(is_loopback));
    let secure_cookies = std::env::var_os("DEMOCRACHAT_SECURE_COOKIES").is_some();
    if !is_loopback && !secure_cookies {
        eprintln!(
            "warning: serving on a non-loopback address without DEMOCRACHAT_SECURE_COOKIES — \
             session cookies will lack the Secure flag. Set it when behind TLS."
        );
    }

    // In dev, give the seeded content accounts a known password so a tester can
    // sign in as them. In production the seed accounts stay passwordless (they
    // author content but cannot be logged into); real users register their own.
    if is_dev {
        rt.block_on(async {
            for who in ["ada", "grace"] {
                let _ = services.set_password(who, DEV_SEED_PASSWORD).await;
            }
        });
        save(&store, &path, data_key.as_ref());
    }

    let served = rt.block_on(async move {
        // Bring federation up (feed + command server + puller) if this node is
        // configured for it; a no-op on the default single-box deployment. Started
        // on this runtime so its background tasks run alongside the web server.
        let nonce_log: Arc<dyn adapter_federation::NonceLog> =
            Arc::new(nonce_log::StoreNonceLog::new(store.clone(), save_hook.clone()));
        let routers =
            federation::start(store, services.clone(), save_hook.clone(), nonce_log, node).await;
        let config = adapter_web::WebConfig {
            addr,
            is_dev,
            secure_cookies,
            signer,
            advance_days: advance,
            save: save_hook,
        };
        adapter_web::serve(services, config, routers).await
    });
    if let Err(e) = served {
        eprintln!("server error: {e}");
        exit(1);
    }
}

/// Serve the web app against Postgres (the production backend, selected by
/// `DATABASE_URL`). Postgres is the source of truth, so there is no JSON snapshot
/// and the save hook is a no-op; federation stays off until its outbox is ported to
/// Postgres (Phase 3). Only the `serve` subcommand is supported — the CLI runs
/// against the local file store.
fn serve_postgres(args: &[String], database_url: String) {
    if args.get(1).map(String::as_str) != Some("serve") {
        eprintln!(
            "error: DATABASE_URL is set, but the Postgres backend serves the web app only. \
             Run `democrachat serve` (the CLI subcommands use the local file store)."
        );
        exit(2);
    }

    let is_dev = args.iter().any(|a| a == "--dev");
    let node = node_id_from_env();
    let addr: SocketAddr = arg_value(args, "--addr")
        .unwrap_or_else(|| "127.0.0.1:3737".to_string())
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("error: bad --addr: {e}");
            exit(2);
        });

    let is_loopback = addr.ip().is_loopback();
    let signer = Arc::new(session_signer_from_env(is_loopback));
    let secure_cookies = std::env::var_os("DEMOCRACHAT_SECURE_COOKIES").is_some();
    if !is_loopback && !secure_cookies {
        eprintln!(
            "warning: serving on a non-loopback address without DEMOCRACHAT_SECURE_COOKIES — \
             session cookies will lack the Secure flag. Set it when behind TLS."
        );
    }

    let media = match adapter_media_fs::FsMediaStore::new(media_dir()) {
        Ok(media) => Arc::new(media),
        Err(e) => {
            eprintln!("error: cannot open media directory {}: {e}", media_dir().display());
            exit(2);
        }
    };
    let image = Arc::new(adapter_image::ReencodingTranscoder::new());
    // Pool ceiling: a burst queues rather than exhausting Postgres. Override with
    // DEMOCRACHAT_PG_MAX_CONNECTIONS.
    let max_connections: u32 = std::env::var("DEMOCRACHAT_PG_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);

    let clock = Arc::new(FixedClock::new(SystemClock.now()));
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let served = rt.block_on(async move {
        let store = match adapter_store_postgres::PgStore::connect(&database_url, max_connections).await {
            Ok(store) => store,
            Err(e) => {
                eprintln!("error: cannot reach Postgres: {e}");
                exit(2);
            }
        };
        let stores = store.as_stores(media, image);
        let services = Arc::new(Services::new(clock.clone(), stores));
        services.backfill_default_channels().await;

        if is_dev {
            seed_if_empty(&services).await;
            for who in ["ada", "grace"] {
                let _ = services.set_password(who, DEV_SEED_PASSWORD).await;
            }
        }

        let advance: Arc<dyn Fn(i64) + Send + Sync> = {
            let c = clock.clone();
            Arc::new(move |days| {
                let now = c.now();
                c.set(now.plus_days(days));
            })
        };
        // Postgres is the source of truth — every write is already durable, so the
        // save hook is a no-op (there is no snapshot to flush).
        let save_hook: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let config = adapter_web::WebConfig {
            addr,
            is_dev,
            secure_cookies,
            signer,
            advance_days: advance,
            save: save_hook.clone(),
        };
        // Bring federation up over the Postgres store if this node is configured for
        // it; a no-op on the default single-box deployment. The outbox producer +
        // consumer, replay cursor, and nonce log are all Postgres-backed now.
        let nonce_log: Arc<dyn adapter_federation::NonceLog> =
            Arc::new(pg_nonce_log::PgNonceLog::new(store.clone()));
        let routers =
            federation::start(store, services.clone(), save_hook, nonce_log, node).await;
        adapter_web::serve(services, config, routers).await
    });
    if let Err(e) = served {
        eprintln!("server error: {e}");
        exit(1);
    }
}

/// The demo password set on seeded accounts in `--dev` mode. 17 chars — clears
/// the 16-char floor. Never used on a real deployment (only seeds get it, only
/// in dev).
const DEV_SEED_PASSWORD: &str = "democrachat-demo!";

/// Build the session signer from `DEMOCRACHAT_SESSION_SECRET`, fail-closed.
///
/// On a **public** bind a missing/placeholder/short secret would let anyone forge
/// `uid.exp.HMAC` cookies and impersonate any account, so the process **exits**.
/// On loopback (dev) we only warn and fall back to a random per-process key.
fn session_signer_from_env(is_loopback: bool) -> SessionSigner {
    match std::env::var("DEMOCRACHAT_SESSION_SECRET") {
        Ok(secret) if secret.starts_with("CHANGE_ME") || secret.len() < 16 => {
            if is_loopback {
                eprintln!(
                    "warning: DEMOCRACHAT_SESSION_SECRET is a placeholder or too short; \
                     using a random per-process key (sessions won't survive a restart)."
                );
                SessionSigner::ephemeral()
            } else {
                eprintln!(
                    "error: refusing to serve on a public address with a placeholder or \
                     <16-char DEMOCRACHAT_SESSION_SECRET — it would make session cookies forgeable."
                );
                exit(2);
            }
        }
        Ok(secret) => SessionSigner::from_secret(&secret),
        Err(_) => {
            if !is_loopback {
                eprintln!(
                    "error: DEMOCRACHAT_SESSION_SECRET is required on a public bind (a random \
                     per-process key would drop every session on restart and can't be shared)."
                );
                exit(2);
            }
            eprintln!(
                "warning: no DEMOCRACHAT_SESSION_SECRET set; using a random per-process key \
                 (sessions won't survive a restart)."
            );
            SessionSigner::ephemeral()
        }
    }
}

/// Pull the value following `flag` out of the argument list, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

/// Give a fresh dataset something to look at: a sample server with a channel and
/// a couple of messages, so the demo isn't a blank page. No-op if any server
/// already exists.
async fn seed_if_empty(services: &Services) {
    if !services.list_servers().await.is_empty() {
        return;
    }
    let _ = services.register_account("ada").await;
    if services.found_server("ada", "Founders Lounge").await.is_ok() {
        // #general is provisioned automatically when the server is founded.
        let _ = services.chat().create_channel("ada", "founders-lounge", "governance", "how we govern ourselves").await;
        let _ = services.chat().post_message("ada", "founders-lounge", "general", "Welcome to democrachat — a chat that governs itself. No owner, no mods: citizens vote.").await;
        let _ = services.chat().post_message("ada", "founders-lounge", "general", "You start as a guest. Join, chat, and once citizens endorse your messages you can earn the vote.").await;

        // Seed a couple of custom emoji so the vote list in server settings has
        // something to rank. Emoji are curated by citizen voting, not ballots.
        // Each url is a self-contained SVG data URI, just like an uploaded image.
        let glyph = |g: &str| {
            format!(
                "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' \
                 width='64' height='64'><text x='6' y='52' font-size='52'>{g}</text></svg>"
            )
        };
        let _ = services.emoji().add_emoji("ada", "founders-lounge", "party", &glyph("🎉")).await;
        let _ = services.emoji().add_emoji("ada", "founders-lounge", "vote", &glyph("🗳️")).await;
    }
    let _ = services.register_account("grace").await;
    let _ = services.join_server("grace", "founders-lounge").await;
    let _ = services.chat().post_message("grace", "founders-lounge", "general", "Hi! I just joined as a member.").await;
}
