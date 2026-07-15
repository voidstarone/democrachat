//! HTTP handlers over the use-cases. Each mutation persists and, where a client
//! view is affected, broadcasts a realtime event.

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use domain::{Tier, Unmet};
use serde_json::json;

use crate::auth::{clear_session_cookie, require_actor, session_cookie};
use crate::dto::*;
use crate::state::AppState;

type Res<T> = Result<Json<T>, (StatusCode, String)>;

fn bad(e: impl ToString) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

fn not_found(what: impl ToString) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, what.to_string())
}

/// Client bootstrap: dev-tools flag, the server clock, and — from the session
/// cookie — who the viewer currently is (so the SPA can restore its identity
/// without trusting a stored handle).
pub async fn config(State(st): State<AppState>, headers: HeaderMap) -> Json<serde_json::Value> {
    let me = crate::auth::current_actor(&st, &headers);
    Json(json!({ "is_dev": st.is_dev, "now": st.services.now().0, "me": me }))
}

/// Log in with a handle + password. On success, sets the signed `sid` session
/// cookie; the returned body carries only the display handle. A wrong handle or
/// password is a single opaque `401` (no account-existence oracle).
pub async fn login(
    State(st): State<AppState>,
    Json(req): Json<CredentialsReq>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let user = st
        .services
        .authenticate(&req.handle, &req.password)
        .ok_or((StatusCode::UNAUTHORIZED, "err.invalid_credentials".to_string()))?;
    let cookie = session_cookie(&st, user.id.0);
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({ "handle": user.handle }))).into_response())
}

/// Register a new account (handle + password), then log it straight in.
pub async fn register(
    State(st): State<AppState>,
    Json(req): Json<CredentialsReq>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let user = st
        .services
        .register_with_password(&req.handle, &req.password)
        .map_err(bad)?;
    st.persist();
    let cookie = session_cookie(&st, user.id.0);
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "handle": user.handle, "is_new": true })),
    )
        .into_response())
}

/// Clear the session cookie.
pub async fn logout(State(st): State<AppState>) -> impl IntoResponse {
    ([(header::SET_COOKIE, clear_session_cookie(&st))], Json(json!({ "ok": true })))
}

/// The signed-in user's own servers — powers their sidebar. A user sees only the
/// servers they belong to; discovering new ones is [`list_public_servers`].
pub async fn list_servers(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Res<Vec<ServerSummary>> {
    let me = require_actor(&st, &headers)?;
    Ok(Json(
        st.services.my_servers(&me).into_iter().map(server_summary).collect(),
    ))
}

/// The public browse directory: every public server. Unauthenticated — discovery is
/// open; private servers never appear here.
pub async fn list_public_servers(State(st): State<AppState>) -> Json<Vec<ServerSummary>> {
    Json(st.services.list_public_servers().into_iter().map(server_summary).collect())
}

pub async fn found_server(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FoundReq>,
) -> Res<ServerSummary> {
    let me = require_actor(&st, &headers)?;
    let g = st
        .services
        .found_server_with_visibility(&me, &req.name, req.is_private)
        .map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "server" }).to_string());
    Ok(Json(ServerSummary {
        phase: "Seed".into(),
        citizens: 1,
        is_private: g.is_private,
        are_invites_open: g.allows_member_invites(),
        slug: g.slug,
        name: g.name,
    }))
}

/// Build the summary DTO for one server row.
fn server_summary((g, phase, citizens): (domain::Server, domain::Phase, u64)) -> ServerSummary {
    ServerSummary {
        phase: format!("{phase:?}"),
        citizens,
        is_private: g.is_private,
        are_invites_open: g.allows_member_invites(),
        slug: g.slug,
        name: g.name,
    }
}

/// Mint an invite code for a server, returning the raw code to share. Members only,
/// and only while the server's invite policy is open.
pub async fn create_invite(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let code = st.services.create_invite(&me, &slug).map_err(bad)?;
    st.persist();
    Ok(Json(json!({ "code": code })))
}

/// The live invite codes for a server (members only).
pub async fn list_invites(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<Vec<InviteDto>> {
    let me = require_actor(&st, &headers)?;
    let invites = st
        .services
        .list_invites(&me, &slug)
        .map_err(bad)?
        .into_iter()
        .map(|i| InviteDto { code_hash: i.code_hash, created_at: i.created_at.0 })
        .collect();
    Ok(Json(invites))
}

/// Revoke an invite by its code digest (members only).
pub async fn revoke_invite(
    State(st): State<AppState>,
    Path((slug, code_hash)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services.revoke_invite(&me, &slug, &code_hash).map_err(bad)?;
    st.persist();
    Ok(Json(json!({ "ok": true })))
}

/// Redeem an invite code: join its server as a member. Returns the joined server's
/// slug so the client can open it.
pub async fn accept_invite(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AcceptInviteReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let m = st.services.accept_invite(&me, &req.code).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "server" }).to_string());
    let slug = st.services.server_slug(m.server_id).unwrap_or_default();
    Ok(Json(json!({ "slug": slug })))
}

pub async fn server_detail(State(st): State<AppState>, Path(slug): Path<String>) -> Res<ServerDetail> {
    let (g, phase, citizens) = st.services.server_snapshot(&slug).ok_or_else(|| not_found("err.no_such_server"))?;
    let founder = st.services.user_handle(g.founder_id).unwrap_or_default();
    let mut surface: Vec<String> = g.enabled_ballots.iter().map(|b| format!("{b:?}")).collect();
    surface.sort();
    let (jury_mode, jury_post, jury_comment) = match g.jury_sizing {
        domain::JurySizing::Sqrt { post_factor_bp, comment_factor_bp } => {
            ("Sqrt", post_factor_bp, comment_factor_bp)
        }
        domain::JurySizing::Proportion { post_bp, comment_bp } => ("Proportion", post_bp, comment_bp),
        domain::JurySizing::Fixed { post, comment } => ("Fixed", post, comment),
    };
    let channels = st
        .services
        .list_channels(&slug)
        .unwrap_or_default()
        .into_iter()
        .map(channel_dto)
        .collect();
    Ok(Json(ServerDetail {
        phase: format!("{phase:?}"),
        citizens,
        founder,
        min_account_age_days: g.criteria.min_account_age_days,
        min_membership_days: g.criteria.min_membership_days,
        min_contribution: g.criteria.min_contribution,
        surface,
        vote_weighting: format!("{:?}", g.vote_weighting),
        weighting_scope: format!("{:?}", g.weighting_scope),
        jury_sizing: JurySizingDto {
            mode: jury_mode.to_string(),
            post: jury_post,
            comment: jury_comment,
        },
        channels,
        is_private: g.is_private,
        are_invites_open: g.allows_member_invites(),
        slug: g.slug,
        name: g.name,
    }))
}

pub async fn join_server(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services.join_server(&me, &slug).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "server" }).to_string());
    Ok(Json(json!({ "ok": true })))
}

pub async fn create_channel(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateChannelReq>,
) -> Res<ChannelDto> {
    let me = require_actor(&st, &headers)?;
    let c = st
        .services
        .create_channel(&me, &slug, &req.name, &req.topic)
        .map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "channel", "server": slug }).to_string());
    Ok(Json(channel_dto(c)))
}

/// Map a domain [`Channel`] to its wire shape, surfacing its encryption state.
fn channel_dto(c: domain::Channel) -> ChannelDto {
    ChannelDto {
        name: c.name,
        topic: c.topic,
        is_encrypted: c.is_encrypted,
        history_mode: match c.history_mode {
            domain::HistoryMode::Open => "open",
            domain::HistoryMode::Ephemeral => "ephemeral",
        }
        .into(),
    }
}

pub async fn enfranchise(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    use app::EnfranchiseOutcome::*;
    let me = require_actor(&st, &headers)?;
    let outcome = st.services.try_enfranchise(&me, &slug).map_err(bad)?;
    st.persist();
    let msg = match &outcome {
        Admitted => {
            st.publish(json!({ "type": "server" }).to_string());
            format!("{me} is now a citizen of {slug}")
        }
        NotEligible(unmet) => format!("Not eligible yet: {}", unmet.iter().map(describe_unmet).collect::<Vec<_>>().join("; ")),
        RateCapped { .. } => "Qualified, but the enfranchisement rate cap is full this window — queued.".into(),
    };
    let is_admitted = matches!(outcome, Admitted);
    Ok(Json(json!({ "is_admitted": is_admitted, "message": msg })))
}

pub async fn my_status(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<MeDto> {
    // A signed-out viewer is simply a guest; no 401 here so the public read view
    // still works.
    let Some(me) = crate::auth::current_actor(&st, &headers) else {
        return Ok(Json(MeDto {
            tier: "guest".into(),
            is_eligible: false,
            unmet: vec!["sign in to participate".into()],
            contribution: 0,
            shares_history: None,
        }));
    };
    match st.services.member_tier(&me, &slug) {
        None => Ok(Json(MeDto {
            tier: "guest".into(),
            is_eligible: false,
            unmet: vec!["not a member — join to start".into()],
            contribution: 0,
            shares_history: None,
        })),
        Some(tier) => {
            let elig = st.services.eligibility(&me, &slug).map_err(bad)?;
            Ok(Json(MeDto {
                tier: match tier {
                    Tier::Guest => "guest",
                    Tier::Member => "member",
                    Tier::Citizen => "citizen",
                }
                .into(),
                is_eligible: elig.is_eligible(),
                unmet: elig.unmet.iter().map(describe_unmet).collect(),
                contribution: st.services.member_contribution(&me, &slug).unwrap_or(0),
                shares_history: st.services.history_sharing(&me, &slug),
            }))
        }
    }
}

/// Set the caller's personal history-sharing preference for a server.
pub async fn set_history_sharing(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(req): Json<HistorySharingReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services.set_history_sharing(&me, &slug, req.shares).map_err(bad)?;
    st.persist();
    Ok(Json(json!({ "ok": true })))
}

pub async fn list_messages(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<Vec<MessageDto>> {
    let viewer = crate::auth::current_actor(&st, &headers).unwrap_or_default();
    let messages = st.services.channel_messages_for(&viewer, &slug, &channel).map_err(bad)?;
    // Chronological, flat (id order) — replies sit at the bottom like everything
    // else and carry a quote of their parent rather than being indented.
    let by_id: std::collections::HashMap<u64, &domain::Message> =
        messages.iter().map(|m| (m.id.0, m)).collect();
    let out = messages
        .iter()
        .map(|m| build_message_dto(&st, &slug, &viewer, m, &by_id))
        .collect();
    Ok(Json(out))
}

/// A one-line excerpt of a body for a reply quote.
fn excerpt(body: &str) -> String {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = flat.chars().take(80).collect();
    if flat.chars().count() > 80 {
        s.push('…');
    }
    s
}

fn build_message_dto(
    st: &AppState,
    slug: &str,
    viewer: &str,
    m: &domain::Message,
    by_id: &std::collections::HashMap<u64, &domain::Message>,
) -> MessageDto {
    // The message being replied to, resolved to a compact quote.
    let is_sealed = m.key_epoch.is_some();
    let reply_to = m.parent.and_then(|pid| by_id.get(&pid.0)).map(|p| ReplyRefDto {
        id: p.id.0,
        author: st.services.user_handle(p.author).unwrap_or_else(|| format!("user{}", p.author)),
        // A sealed parent's body is ciphertext — quote it as a lock, not gibberish.
        excerpt: if p.is_deleted {
            "[deleted]".into()
        } else if p.key_epoch.is_some() {
            "🔒 encrypted".into()
        } else {
            excerpt(&p.body)
        },
    });
    // A reply pings the author of the message it answers.
    let replies_to_viewer = !viewer.is_empty()
        && reply_to.as_ref().is_some_and(|r| r.author == viewer);

    // Mentions can only be resolved on plaintext — a sealed body is opaque here, so
    // its @mentions (if any) are resolved client-side after decryption.
    let (mentions, mentions_me) = if m.is_deleted || is_sealed {
        (Vec::new(), replies_to_viewer)
    } else {
        let resolved = st.services.resolve_mentions(slug, &m.body);
        let mentions_me = replies_to_viewer
            || (!viewer.is_empty() && resolved.iter().any(|r| r.handles.iter().any(|h| h == viewer)));
        let dtos = resolved
            .into_iter()
            .filter(|r| r.is_resolved())
            .map(|r| MentionDto { token: r.token, kind: r.kind.as_str().into() })
            .collect();
        (dtos, mentions_me)
    };

    MessageDto {
        id: m.id.0,
        author: st.services.user_handle(m.author).unwrap_or_else(|| format!("user{}", m.author)),
        body: if m.is_deleted { "[deleted]".into() } else { m.body.clone() },
        key_epoch: m.key_epoch,
        ts: m.created_at.0,
        parent: m.parent.map(|p| p.0),
        is_deleted: m.is_deleted,
        edited: m.edited_at.is_some(),
        reactions: st.services.message_reactions(m.id.0),
        mentions,
        mentions_me,
        reply_to,
        attachments: m
            .attachments
            .iter()
            .map(|a| AttachmentDto {
                url: format!("/media/{}", a.key),
                content_type: a.content_type.clone(),
                kind: a.kind.as_str().into(),
                caption: a.caption.clone(),
                is_spoiler: a.is_spoiler,
            })
            .collect(),
    }
}

pub async fn post_message(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<PostReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    // Build domain attachments, deriving the kind from the (base) content type and
    // dropping anything that isn't a supported media type.
    let attachments: Vec<domain::Attachment> = req
        .attachments
        .iter()
        .filter_map(|a| {
            let base = a.content_type.split(';').next().unwrap_or(&a.content_type).trim();
            domain::MediaKind::from_content_type(base).map(|kind| {
                domain::Attachment::new(a.key.clone(), base.to_string(), kind, a.caption.clone(), a.is_spoiler)
            })
        })
        .collect();
    let msg = match req.parent {
        Some(parent) => st.services.reply_message(&me, parent, &req.body).map_err(bad)?,
        None if attachments.is_empty() => {
            st.services.post_message(&me, &slug, &channel, &req.body).map_err(bad)?
        }
        None => st
            .services
            .post_message_with_attachments(&me, &slug, &channel, &req.body, attachments)
            .map_err(bad)?,
    };
    st.persist();
    st.publish(json!({ "type": "message", "server": slug, "channel": channel }).to_string());
    Ok(Json(json!({ "id": msg.id.0 })))
}

/// Upload a media blob (image/video/audio). Requires a signed-in caller. The raw
/// request body is the file bytes; the `Content-Type` header is its MIME type.
/// Returns the storage key + type for the client to attach to a message.
pub async fn upload_media(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Res<serde_json::Value> {
    let _me = require_actor(&st, &headers)?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let (key, kind) = st.services.store_media(content_type, &body).map_err(bad)?;
    let base = content_type.split(';').next().unwrap_or(content_type).trim();
    Ok(Json(json!({ "key": key, "content_type": base, "kind": kind.as_str() })))
}

/// Serve a stored media blob by key. Public (keys are opaque and unguessable), with
/// long-lived immutable caching and `nosniff` so the browser honours our type.
pub async fn serve_media(
    State(st): State<AppState>,
    Path(key): Path<String>,
) -> axum::response::Response {
    match st.services.media_blob(&key) {
        Some((content_type, bytes)) => {
            let ct = axum::http::HeaderValue::from_str(&content_type)
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream"));
            (
                [
                    (header::CONTENT_TYPE, ct),
                    (
                        header::CACHE_CONTROL,
                        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
                    ),
                    (
                        header::X_CONTENT_TYPE_OPTIONS,
                        axum::http::HeaderValue::from_static("nosniff"),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "err.no_such_media").into_response(),
    }
}

/// Post an end-to-end-encrypted message: the client already sealed the body under
/// the channel key of `key_epoch`, so the server stores only ciphertext.
pub async fn post_sealed_message(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<PostSealedReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let msg = st
        .services
        .post_sealed_message(&me, &slug, &channel, &req.ciphertext, req.key_epoch, req.parent)
        .map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "message", "server": slug, "channel": channel }).to_string());
    Ok(Json(json!({ "id": msg.id.0 })))
}

/// Turn on end-to-end encryption for a channel (citizen-only). The channel key is
/// minted and granted to members client-side afterwards.
pub async fn enable_channel_encryption(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<EnableEncryptionReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let mode = match req.history_mode.trim() {
        "ephemeral" => domain::HistoryMode::Ephemeral,
        _ => domain::HistoryMode::Open,
    };
    st.services.enable_channel_encryption(&me, &slug, &channel, mode).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "channel", "server": slug }).to_string());
    Ok(Json(json!({ "ok": true })))
}

/// Publish a sealed channel-key grant for another member (the caller sealed the
/// key to that member's device key client-side).
pub async fn grant_channel_key(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<GrantKeyReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services
        .grant_channel_key(&me, &slug, &channel, req.epoch, &req.member, &req.sealed_key)
        .map_err(bad)?;
    st.persist();
    Ok(Json(json!({ "ok": true })))
}

/// The caller's own key grants for a channel, so their client can open sealed
/// messages. One entry per epoch they've been granted.
pub async fn my_channel_grants(
    State(st): State<AppState>,
    Path((slug, channel)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<Vec<ChannelGrantDto>> {
    let me = require_actor(&st, &headers)?;
    let grants = st
        .services
        .my_channel_grants(&me, &slug, &channel)
        .map_err(bad)?
        .into_iter()
        .map(|g| ChannelGrantDto { epoch: g.epoch, sealed_key: g.sealed_key })
        .collect();
    Ok(Json(grants))
}

pub async fn react(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
    Json(req): Json<ReactReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services.react(&me, id, &req.emoji).map_err(bad)?;
    st.persist();
    if let Some((server, channel)) = st.services.message_context(id) {
        st.publish(json!({ "type": "reaction", "server": server, "channel": channel }).to_string());
    }
    Ok(Json(json!({ "ok": true })))
}

/// List a server's proposals (resolving any that are due first).
pub async fn list_proposals(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<Vec<ProposalDto>> {
    let me = crate::auth::current_actor(&st, &headers).unwrap_or_default();
    let now = st.services.now().0;
    let dtos = st
        .services
        .list_proposals(&slug)
        .into_iter()
        .map(|p| {
            let (aye, nay) = st.services.proposal_head_counts(p.id.0);
            let (status, closes_in, is_applied) = match p.status {
                domain::ProposalStatus::Open => ("open", Some(p.closes_at.0 - now), false),
                domain::ProposalStatus::Passed { .. } => ("passed", None, p.is_applied),
                domain::ProposalStatus::Failed => ("failed", None, false),
            };
            ProposalDto {
                id: p.id.0,
                summary: summarize_kind(&st, &p.kind),
                proposer: st.services.user_handle(p.proposer).unwrap_or_default(),
                status: status.into(),
                aye,
                nay,
                closes_in,
                my_vote: st.services.my_vote(p.id.0, &me),
                is_applied,
            }
        })
        .collect();
    Ok(Json(dtos))
}

/// Open a proposal. Fails if the server doesn't govern it or the proposer isn't a citizen.
pub async fn propose(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ProposeReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let kind = to_proposal_kind(&st, &slug, &req.kind).map_err(bad)?;
    let p = st.services.open_proposal(&me, &slug, kind).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "proposal", "server": slug }).to_string());
    Ok(Json(json!({ "id": p.id.0 })))
}

/// Cast (or change) a vote on a proposal.
pub async fn vote(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
    Json(req): Json<VoteReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    match &st.vote_router {
        // Federated: route to the node that owns the proposal's server (which may be
        // this one). The owner re-checks citizenship and mints the canonical event.
        Some(router) => {
            let voter_id = st.services.user_id(&me).ok_or_else(|| bad("err.no_such_account"))?;
            router.cast_vote(voter_id, id, req.is_aye).await.map_err(bad)?;
        }
        // Single-box: apply directly.
        None => st.services.cast_vote(&me, id, req.is_aye).map_err(bad)?,
    }
    st.persist();
    // A proposal doesn't carry its server slug here; broadcast a generic nudge.
    st.publish(json!({ "type": "proposal" }).to_string());
    Ok(Json(json!({ "ok": true })))
}

/// The server's custom-emoji vote list: active + considered, highest net score
/// first. Only currently-franchised citizens' votes count.
pub async fn list_emojis(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<Vec<EmojiDto>> {
    let me = crate::auth::current_actor(&st, &headers).unwrap_or_default();
    let dtos = st
        .services
        .ranked_emojis(&slug, &me)
        .into_iter()
        .map(|e| EmojiDto {
            id: e.id,
            name: e.name,
            url: e.url,
            score: e.score,
            standing: match e.standing {
                domain::EmojiStanding::Active => "active",
                domain::EmojiStanding::Considered => "considered",
                domain::EmojiStanding::Archived => "archived",
            }
            .into(),
            is_active: e.standing.is_active(),
            my_vote: e.my_vote,
        })
        .collect();
    Ok(Json(dtos))
}

/// Every retained emoji's `name → url` for a server, so the client can render
/// `:name:` shortcodes in any message — including archived emoji from old posts.
pub async fn emoji_map(State(st): State<AppState>, Path(slug): Path<String>) -> Res<serde_json::Value> {
    let map: serde_json::Map<String, serde_json::Value> = st
        .services
        .emoji_name_map(&slug)
        .into_iter()
        .map(|(name, url)| (name, serde_json::Value::String(url)))
        .collect();
    Ok(Json(serde_json::Value::Object(map)))
}

/// Add a custom emoji (citizen-only). An uploaded image is validated and inlined
/// as a `data:` URI here; otherwise an external `url` is used as-is.
pub async fn add_emoji(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(req): Json<AddEmojiReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    let url = emoji_url(&req.url, &req.image).map_err(bad)?;
    let emoji = st.services.add_emoji(&me, &slug, &req.name, &url).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "emoji", "server": slug }).to_string());
    Ok(Json(json!({ "id": emoji.id.0, "name": emoji.name })))
}

/// Cast (or change) a citizen's up/down vote on a custom emoji.
pub async fn vote_emoji(
    State(st): State<AppState>,
    Path((slug, id)): Path<(String, u64)>,
    headers: HeaderMap,
    Json(req): Json<VoteEmojiReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services.vote_emoji(&me, &slug, id, req.is_up).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "emoji", "server": slug }).to_string());
    Ok(Json(json!({ "ok": true })))
}

/// Resolve a custom emoji's image reference. An uploaded `image` (base64) wins: it
/// is decoded, validated against the platform rules (≤256×256 PNG/GIF, size cap —
/// [`domain::validate_emoji_image`]), and turned into a self-contained `data:` URI.
/// Otherwise an external `url` is used as-is.
fn emoji_url(url: &str, image: &Option<String>) -> Result<String, String> {
    use base64::Engine;
    match image {
        Some(b64) if !b64.trim().is_empty() => {
            let b64 = b64.trim();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|_| "err.emoji_image_not_base64".to_string())?;
            let format = domain::validate_emoji_image(&bytes).map_err(|e| e.to_string())?;
            Ok(format!("data:{};base64,{}", format.mime(), b64))
        }
        _ => {
            let url = url.trim();
            if url.is_empty() {
                return Err("err.emoji_image_required".into());
            }
            Ok(url.to_string())
        }
    }
}

/// Publish the calling user's device keys — the public key plus the
/// password-wrapped secret. The identity is generated and wrapped **client-side**;
/// the server only stores the opaque blob and validates the public key's shape.
pub async fn publish_keys(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PublishKeysReq>,
) -> Res<serde_json::Value> {
    let me = require_actor(&st, &headers)?;
    st.services
        .publish_keys(&me, &req.public_key, req.wrapped_secret.into())
        .map_err(bad)?;
    st.persist();
    Ok(Json(json!({ "ok": true })))
}

/// Hand the calling user back their own directory entry, including the wrapped
/// secret, so a new device can unwrap it with the password. Served only to the
/// authenticated owner — the actor comes from the session, never the request.
pub async fn my_keys(State(st): State<AppState>, headers: HeaderMap) -> Res<MyKeysDto> {
    let me = require_actor(&st, &headers)?;
    let keys = st.services.my_keys(&me).map_err(bad)?;
    Ok(Json(MyKeysDto {
        public_key: keys.public_key,
        wrapped_secret: keys.wrapped_secret.into(),
    }))
}

/// Another user's **public** key, for sealing a DM or message-key to them. Any
/// signed-in user may fetch it; it is not secret. Never exposes a wrapped secret.
pub async fn public_key(
    State(st): State<AppState>,
    Path(handle): Path<String>,
    headers: HeaderMap,
) -> Res<PublicKeyDto> {
    require_actor(&st, &headers)?;
    let public_key = st.services.public_key_of(&handle).map_err(bad)?;
    Ok(Json(PublicKeyDto { handle, public_key }))
}

/// Translate the client's tagged request into a domain [`ProposalKind`],
/// resolving handles/ids that only the server can look up.
fn to_proposal_kind(
    st: &AppState,
    slug: &str,
    req: &ProposeKindReq,
) -> Result<domain::ProposalKind, String> {
    use domain::ProposalKind as K;
    Ok(match req {
        ProposeKindReq::AddRule { text } => K::AddRule { text: text.clone() },
        ProposeKindReq::RemoveRule { rule } => K::RemoveRule { rule: domain::RuleId(*rule) },
        ProposeKindReq::CreateChannel { name, topic } => {
            K::CreateChannel { name: name.clone(), topic: topic.clone() }
        }
        ProposeKindReq::DeleteChannel { name } => K::DeleteChannel { name: name.clone() },
        ProposeKindReq::Ban { handle } => {
            let u = st.services.find_user(handle).ok_or_else(|| format!("no such user: {handle}"))?;
            K::Ban { user: u.id }
        }
        ProposeKindReq::CreateRole { name } => K::CreateRole { name: name.clone() },
        ProposeKindReq::DeleteRole { role } => K::DeleteRole { role: resolve_role(st, slug, role)? },
        ProposeKindReq::AssignRole { handle, role } => K::AssignRole {
            user: resolve_member(st, slug, handle)?,
            role: resolve_role(st, slug, role)?,
        },
        ProposeKindReq::UnassignRole { handle, role } => K::UnassignRole {
            user: resolve_member(st, slug, handle)?,
            role: resolve_role(st, slug, role)?,
        },
        ProposeKindReq::SetRehomingPolicy { is_disabled } => {
            K::SetRehomingPolicy { is_disabled: *is_disabled }
        }
        ProposeKindReq::SetInvitePolicy { is_open } => K::SetInvitePolicy {
            policy: if *is_open {
                domain::InvitePolicy::Open
            } else {
                domain::InvitePolicy::Closed
            },
        },
        ProposeKindReq::AmendCriteria {
            min_account_age_days,
            min_membership_days,
            min_contribution,
        } => K::AmendCriteria {
            proposed: domain::FranchiseCriteria {
                min_account_age_days: *min_account_age_days,
                min_membership_days: *min_membership_days,
                min_contribution: *min_contribution,
            },
        },
        ProposeKindReq::SetVoteWeighting { scheme } => K::SetVoteWeighting {
            scheme: domain::VoteWeighting::from_name(scheme)
                .ok_or_else(|| format!("unknown vote weighting: {scheme}"))?,
        },
        ProposeKindReq::SetWeightingScope { scope } => K::SetWeightingScope {
            scope: domain::WeightingScope::from_name(scope)
                .ok_or_else(|| format!("unknown weighting scope: {scope}"))?,
        },
        ProposeKindReq::SetJurySizing { mode, post, comment } => K::SetJurySizing {
            sizing: match mode.as_str() {
                "Sqrt" => domain::JurySizing::Sqrt {
                    post_factor_bp: *post,
                    comment_factor_bp: *comment,
                },
                "Proportion" => domain::JurySizing::Proportion { post_bp: *post, comment_bp: *comment },
                "Fixed" => domain::JurySizing::Fixed { post: *post, comment: *comment },
                other => return Err(format!("unknown jury sizing mode: {other}")),
            },
        },
        ProposeKindReq::SetGovernanceSurface { kinds } => K::SetGovernanceSurface {
            enabled: kinds.iter().filter_map(|k| domain::BallotKind::from_name(k)).collect(),
        },
    })
}

/// Resolve a handle to a member's [`UserId`], erroring if unknown.
fn resolve_member(st: &AppState, _slug: &str, handle: &str) -> Result<domain::UserId, String> {
    st.services.find_user(handle).map(|u| u.id).ok_or_else(|| format!("no such user: {handle}"))
}

/// Resolve a role name to its [`RoleId`] within a server, erroring if unknown.
fn resolve_role(st: &AppState, slug: &str, name: &str) -> Result<domain::RoleId, String> {
    let norm = domain::normalize_role_name(name);
    st.services
        .list_roles(slug)
        .into_iter()
        .find(|r| r.name == norm)
        .map(|r| r.id)
        .ok_or_else(|| format!("no such role: {name}"))
}

/// A short human summary of a proposal for the list.
fn summarize_kind(st: &AppState, kind: &domain::ProposalKind) -> String {
    use domain::ProposalKind as K;
    match kind {
        K::AddRule { text } => format!("Add rule: “{text}”"),
        K::RemoveRule { rule } => format!("Repeal rule #{}", rule.0),
        K::CreateChannel { name, .. } => format!("Create channel #{name}"),
        K::DeleteChannel { name } => format!("Delete channel #{name}"),
        K::Ban { user } => format!("Ban @{}", st.services.user_handle(*user).unwrap_or_default()),
        K::Timeout { user, .. } => format!("Time out @{}", st.services.user_handle(*user).unwrap_or_default()),
        K::Recall { leader } => format!("Recall @{}", st.services.user_handle(*leader).unwrap_or_default()),
        K::AmendCriteria { .. } => "Amend franchise criteria".into(),
        K::SetJurySizing { .. } => "Change jury sizing".into(),
        K::SetVoteWeighting { .. } => "Change vote weighting".into(),
        K::SetWeightingScope { .. } => "Change weighting scope".into(),
        K::GrantVoteWeight { user, weight } => {
            format!("Grant @{} vote weight {weight}", st.services.user_handle(*user).unwrap_or_default())
        }
        K::SetGovernanceSurface { .. } => "Change what this server votes on".into(),
        K::RemoveContent { target } => format!("Remove content {target}"),
        K::CreateRole { name } => format!("Create role @{name}"),
        K::DeleteRole { role } => format!("Delete role #{}", role.0),
        K::AssignRole { user, role } => format!(
            "Add @{} to role #{}",
            st.services.user_handle(*user).unwrap_or_default(),
            role.0
        ),
        K::UnassignRole { user, role } => format!(
            "Remove @{} from role #{}",
            st.services.user_handle(*user).unwrap_or_default(),
            role.0
        ),
        K::SetRehomingPolicy { is_disabled } => if *is_disabled {
            "Disable server rehoming (pin to home node)"
        } else {
            "Enable server rehoming"
        }
        .into(),
        K::SetInvitePolicy { policy } => match policy {
            domain::InvitePolicy::Open => "Open invites (any member may invite)".into(),
            domain::InvitePolicy::Closed => "Close invites (seal admission by link)".into(),
        },
    }
}

/// List a server's custom roles and their holders (for the governance panel).
pub async fn list_roles(State(st): State<AppState>, Path(slug): Path<String>) -> Res<Vec<RoleDto>> {
    let dtos = st
        .services
        .list_roles(&slug)
        .into_iter()
        .map(|r| RoleDto {
            id: r.id.0,
            holders: st.services.role_holders(&slug, &r.name),
            name: r.name,
        })
        .collect();
    Ok(Json(dtos))
}

/// The names a client may `@mention` on a server: members and roles. Powers the
/// composer's autocomplete.
pub async fn mentionable(State(st): State<AppState>, Path(slug): Path<String>) -> Res<MentionableDto> {
    Ok(Json(MentionableDto {
        users: st.services.member_handles(&slug),
        roles: st.services.mentionable_role_names(&slug),
    }))
}

/// Dev-only: simulate enough citizen endorsements to clear the contribution bar,
/// so the "become a citizen" flow is reachable in a fresh demo (a real server
/// needs many distinct citizens to react). Honest about being a shortcut.
pub async fn dev_endorse(
    State(st): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    if !st.is_dev {
        return Err((StatusCode::FORBIDDEN, "err.dev_disabled".into()));
    }
    let me = require_actor(&st, &headers)?;
    let current = st.services.member_contribution(&me, &slug).unwrap_or(0);
    st.services.set_contribution(&me, &slug, current + 5).map_err(bad)?;
    st.persist();
    st.publish(json!({ "type": "server" }).to_string());
    Ok(Json(json!({ "ok": true })))
}

pub async fn advance_clock(
    State(st): State<AppState>,
    Json(req): Json<AdvanceReq>,
) -> Res<serde_json::Value> {
    if !st.is_dev {
        return Err((StatusCode::FORBIDDEN, "err.dev_disabled".into()));
    }
    (st.advance_days)(req.days);
    st.publish(json!({ "type": "clock" }).to_string());
    Ok(Json(json!({ "now": st.services.now().0 })))
}

fn describe_unmet(u: &Unmet) -> String {
    match u {
        Unmet::AccountTooYoung { need_days, have_days } => {
            format!("account age {have_days}d / {need_days}d")
        }
        Unmet::MembershipTooShort { need_days, have_days } => {
            format!("membership {have_days}d / {need_days}d")
        }
        Unmet::InsufficientContribution { need, have } => {
            format!("endorsed contribution {have} / {need}")
        }
        Unmet::Sanctioned => "under an active sanction".into(),
        Unmet::Barred => "barred from the franchise".into(),
    }
}
