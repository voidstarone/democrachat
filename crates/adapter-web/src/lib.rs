//! democrachat web adapter — a JS-first realtime client over the use-cases.
//!
//! A single-page app (served from [`INDEX_HTML`]) talks to a small JSON API and
//! subscribes to a WebSocket for live updates. The composition root injects the
//! wired [`Services`], a clock-advance hook (for the `--dev` demo), and a save
//! hook; this crate names no store.

mod auth;
mod dto;
mod handlers;
mod middleware;
mod signal;
mod social_handlers;
mod state;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use app::{Services, SessionSigner};
use axum::http::header;
use axum::response::Html;
use axum::routing::{delete, get, post};
use axum::Router;
use tokio::sync::broadcast;

use state::AppState;

const INDEX_HTML: &str = include_str!("index.html");
const APP_JS: &str = include_str!("app.js");
/// Message catalogs for the client, one per supported locale. Baked in and
/// prepended to `app.js` (see the `/app.js` route) so the SPA has its strings
/// synchronously at first paint — no extra fetch, no flash of untranslated keys.
const LOCALE_EN: &str = include_str!("locales/en.json");
const LOCALE_ES: &str = include_str!("locales/es.json");
/// The wasm-bindgen JS glue and the compiled WebAssembly for the E2EE crypto
/// ([`e2ee-wasm`]). Baked into the binary and served from origin so the browser
/// runs the exact same crypto as the CLI and the server-side tests.
const E2EE_JS: &str = include_str!("wasm/e2ee.js");
const E2EE_WASM: &[u8] = include_bytes!("wasm/e2ee_bg.wasm");

/// The optional federation write-routers the web layer installs. `None` on a
/// single-box deployment, where every write applies locally; `Some` when a control
/// plane forwards cross-scope writes to their owner.
#[derive(Default, Clone)]
pub struct Routers {
    pub vote: Option<Arc<dyn app::VoteRouter>>,
    pub dm: Option<Arc<dyn app::DmRouter>>,
    pub block: Option<Arc<dyn app::BlockRouter>>,
    pub friend: Option<Arc<dyn app::FriendRouter>>,
}

/// Everything the composition root wires into the server besides the use-cases and
/// the router bundle: the bind address, deployment flags, the session signer, and
/// the dev-only clock-advance and save hooks.
pub struct WebConfig {
    pub addr: SocketAddr,
    pub is_dev: bool,
    pub secure_cookies: bool,
    pub signer: Arc<SessionSigner>,
    pub advance_secs: Arc<dyn Fn(i64) + Send + Sync>,
    pub save: Arc<dyn Fn() + Send + Sync>,
}

/// Serve the web app until the process is stopped.
pub async fn serve(
    services: Arc<Services>,
    config: WebConfig,
    routers: Routers,
) -> anyhow::Result<()> {
    let WebConfig { addr, is_dev, secure_cookies, signer, advance_secs, save } = config;
    let (events, _) = broadcast::channel(256);
    let state = AppState {
        services,
        events,
        signal: Arc::new(signal::SignalHub::default()),
        signer,
        secure_cookies,
        is_dev,
        advance_secs,
        save,
        vote_router: routers.vote,
        dm_router: routers.dm,
        block_router: routers.block,
        friend_router: routers.friend,
    };

    let limiter = Arc::new(middleware::rate_limit::RateLimiter::new());

    let app = Router::new()
        // `no-store` so the browser always fetches the current single-page app —
        // a stale cached page would break the realtime wiring.
        .route(
            "/",
            get(|| async { ([(header::CACHE_CONTROL, "no-store")], Html(INDEX_HTML)) }),
        )
        // The SPA's script, served from origin so the CSP can be `script-src 'self'`
        // (no inline handlers). `no-store` keeps it in lockstep with the shell.
        .route(
            "/app.js",
            get(|| async {
                // Prepend the message catalogs as a global so `t()` resolves strings
                // synchronously before any UI renders. The locale files are valid
                // JSON, so they embed directly as object values.
                let body = format!(
                    "window.__CATALOGS__={{\"en\":{LOCALE_EN},\"es\":{LOCALE_ES}}};\n{APP_JS}"
                );
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "no-store"),
                    ],
                    body,
                )
            }),
        )
        // The E2EE crypto module: wasm-bindgen glue (an ES module the SPA imports)
        // and the WebAssembly it loads. The glue fetches the `.wasm` relative to its
        // own URL, so both live under `/wasm/`.
        .route(
            "/wasm/e2ee.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "no-store"),
                    ],
                    E2EE_JS,
                )
            }),
        )
        .route(
            "/wasm/e2ee_bg.wasm",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "application/wasm"),
                        (header::CACHE_CONTROL, "no-store"),
                    ],
                    E2EE_WASM,
                )
            }),
        )
        .route("/ws", get(ws::ws_handler))
        .route("/api/config", get(handlers::config))
        .route("/api/login", post(handlers::login))
        .route("/api/register", post(handlers::register))
        .route("/api/logout", post(handlers::logout))
        .route("/api/servers", get(handlers::list_servers).post(handlers::found_server))
        .route("/api/servers/public", get(handlers::list_public_servers))
        .route("/api/invites/accept", post(handlers::accept_invite))
        .route("/api/servers/:slug", get(handlers::server_detail))
        .route("/api/servers/:slug/join", post(handlers::join_server))
        .route(
            "/api/servers/:slug/invites",
            get(handlers::list_invites).post(handlers::create_invite),
        )
        .route("/api/servers/:slug/invites/:code_hash", delete(handlers::revoke_invite))
        .route("/api/servers/:slug/channels", post(handlers::create_channel))
        .route("/api/servers/:slug/tags", post(handlers::set_server_tags))
        .route("/api/servers/:slug/channels/:channel/tags", post(handlers::set_channel_tags))
        .route("/api/search/servers", get(handlers::search_servers_by_tag))
        .route("/api/search/channels", get(handlers::search_channels_by_tag))
        .route("/api/search/users", get(handlers::search_users_by_tag))
        .route("/api/servers/:slug/enfranchise", post(handlers::enfranchise))
        .route("/api/servers/:slug/me", get(handlers::my_status))
        .route("/api/servers/:slug/history-sharing", post(handlers::set_history_sharing))
        .route("/api/servers/:slug/moderator-optout", post(handlers::set_moderator_optout))
        .route(
            "/api/servers/:slug/channels/:channel/messages",
            get(handlers::list_messages).post(handlers::post_message),
        )
        // Media upload (raw body) needs a far larger body cap than the JSON routes,
        // so it carries its own inner limit that overrides the global one. Serving
        // is public and unauthenticated — keys are opaque.
        .route(
            "/api/media",
            post(handlers::upload_media)
                .layer(axum::extract::DefaultBodyLimit::max(MAX_MEDIA_UPLOAD_BYTES)),
        )
        .route("/media/:key", get(handlers::serve_media))
        // Encrypted channels: post a sealed message, turn encryption on, distribute
        // and fetch the per-member channel-key grants.
        .route(
            "/api/servers/:slug/channels/:channel/sealed-messages",
            post(handlers::post_sealed_message),
        )
        .route(
            "/api/servers/:slug/channels/:channel/encrypt",
            post(handlers::enable_channel_encryption),
        )
        .route(
            "/api/servers/:slug/channels/:channel/keys",
            get(handlers::my_channel_grants).post(handlers::grant_channel_key),
        )
        .route("/api/messages/:id/react", post(handlers::react))
        .route("/api/servers/:slug/roles", get(handlers::list_roles))
        .route("/api/servers/:slug/roles/:id/color", post(handlers::vote_role_color))
        .route("/api/servers/:slug/members/:handle/roles", get(handlers::user_roles))
        .route("/api/servers/:slug/members", get(handlers::list_members))
        .route("/api/servers/:slug/active", get(handlers::active_members))
        .route("/api/servers/:slug/search", get(handlers::search_messages))
        .route("/api/servers/:slug/mute", post(handlers::mute))
        .route("/api/servers/:slug/unmute", post(handlers::unmute))
        .route("/api/servers/:slug/mentionable", get(handlers::mentionable))
        .route(
            "/api/servers/:slug/proposals",
            get(handlers::list_proposals).post(handlers::propose),
        )
        .route("/api/proposals/:id/vote", post(handlers::vote))
        .route("/api/proposals/:id/amend", post(handlers::amend))
        .route(
            "/api/proposals/:id/discussion",
            get(handlers::list_discussion).post(handlers::post_discussion),
        )
        // Custom emoji: the ranked vote list lives in server settings; the name→url
        // map lets the client render `:name:` shortcodes in any message.
        .route(
            "/api/servers/:slug/emojis",
            get(handlers::list_emojis).post(handlers::add_emoji),
        )
        .route("/api/servers/:slug/emojis/map", get(handlers::emoji_map))
        .route("/api/servers/:slug/emojis/:id/vote", post(handlers::vote_emoji))
        // Server-blind key directory: publish own keys, fetch own wrapped secret,
        // fetch any user's public key (to seal a DM / message-key to them).
        .route("/api/keys", post(handlers::publish_keys))
        .route("/api/keys/me", get(handlers::my_keys))
        .route("/api/keys/:handle", get(handlers::public_key))
        .route("/api/servers/:slug/dev/endorse", post(handlers::dev_endorse))
        .route("/api/dev/advance", post(handlers::advance_clock))
        // Social layer: DMs, blocks, friendships. `:me` is the acting user.
        .route("/api/social/:me", get(social_handlers::social_me))
        .route("/api/social/:me/policy", post(social_handlers::set_policy))
        .route("/api/social/:me/tags", post(social_handlers::set_tags))
        .route("/api/social/:me/with/:other", get(social_handlers::conversation))
        .route("/api/social/:me/dm/:other", post(social_handlers::send_dm))
        .route("/api/social/:me/block/:other", post(social_handlers::block))
        .route("/api/social/:me/friend/:other", post(social_handlers::request_friend))
        .route("/api/social/:me/accept/:other", post(social_handlers::accept_friend))
        .with_state(state)
        // Universal layers (outermost first): rate-limit → CSRF → security headers
        // → body-size cap. A throttled or forged request is rejected before any
        // handler or expensive work runs.
        .layer(axum::middleware::from_fn(middleware::security_headers::security_headers))
        .layer(axum::middleware::from_fn(middleware::csrf::csrf))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            middleware::rate_limit::rate_limit,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("democrachat web: http://{addr}{}", if is_dev { "  (dev clock enabled)" } else { "" });
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Cap on request-body size. The app takes only small JSON — no uploads — so a
/// tight limit turns a memory-exhaustion attempt into a `413`.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Body cap for the media-upload route only (26 MiB) — a hair above the app's
/// 25 MiB per-file limit, so an oversized file gets a clean domain error rather
/// than a raw `413`. Applied as a route-local override of [`MAX_BODY_BYTES`].
const MAX_MEDIA_UPLOAD_BYTES: usize = 26 * 1024 * 1024;
