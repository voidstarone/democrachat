//! HTTP handlers for the social layer: direct messages, blocks, and friendships.
//!
//! The acting user (`me`) is named in the path, so a mutation reads as
//! `POST /api/social/:me/block/:other`. Every DM/social change broadcasts a
//! `social` event carrying the two handles, so an open conversation refreshes
//! live for whichever browser is viewing it.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use domain::DmPolicy;
use serde_json::json;

use crate::auth::require_actor;
use crate::dto::*;
use crate::state::AppState;

type Res<T> = Result<Json<T>, (StatusCode, String)>;

fn bad(e: impl ToString) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}

/// Require that the authenticated actor **is** `me` from the path. DMs, blocks,
/// friendships and social state are private to their owner, so you may only ever
/// read or act as yourself. This is what closes the DM-IDOR hole — a signed-in
/// `@bob` cannot pass `@alice` in the path to read her conversations.
fn require_self(st: &AppState, headers: &HeaderMap, me: &str) -> Result<(), (StatusCode, String)> {
    let actor = require_actor(st, headers)?;
    if actor == me {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "err.forbidden_not_self".to_string()))
    }
}

/// A social event naming the two parties, so only the affected browsers refetch.
fn social_event(a: &str, b: &str) -> String {
    json!({ "type": "social", "a": a, "b": b }).to_string()
}

/// The viewer's whole social state: DM sidebar, friends, incoming requests, policy.
pub async fn social_me(
    State(st): State<AppState>,
    Path(me): Path<String>,
    headers: HeaderMap,
) -> Res<SocialMeDto> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    if s.find_user(&me).is_none() {
        return Err((StatusCode::NOT_FOUND, "err.no_such_user".into()));
    }

    let friends = s.social().friends_of(&me);
    let partners = s.social()
        .dm_partners(&me)
        .into_iter()
        .map(|handle| DmPartnerDto {
            is_friend: friends.iter().any(|f| f == &handle),
            is_blocked: s.social().is_blocked_between(&me, &handle),
            can_dm: s.social().can_dm(&me, &handle),
            handle,
        })
        .collect();

    Ok(Json(SocialMeDto {
        is_friends_only: matches!(s.social().dm_policy(&me), Some(DmPolicy::FriendsOnly)),
        partners,
        friends,
        incoming_requests: s.social().incoming_friend_requests(&me),
        blocked: s.social().blocked_users(&me),
        handle: me,
    }))
}

/// The conversation between `me` and `other`, oldest first.
pub async fn conversation(
    State(st): State<AppState>,
    Path((me, other)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<Vec<DmMessageDto>> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    let me_user = s.find_user(&me).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".to_string()))?;
    let out = s.social()
        .conversation(&me, &other)
        .into_iter()
        .map(|m| {
            let is_mine = m.sender == me_user.id;
            // Hand the viewer only the ciphertext they hold a key for.
            let sealed_for_me =
                if is_mine { m.sealed_for_sender } else { m.sealed_for_recipient };
            DmMessageDto {
                id: m.id.0,
                is_mine,
                sender: s.chat().user_handle(m.sender).unwrap_or_default(),
                recipient: s.chat().user_handle(m.recipient).unwrap_or_default(),
                sealed_for_me,
            }
        })
        .collect();
    Ok(Json(out))
}

/// Send a DM from `me` to `other`.
///
/// When federated, the DM is owned by the **sender's** home node, so it goes
/// through the [`DmRouter`](app::DmRouter) — applied locally if we own the sender,
/// forwarded otherwise. The owner mints the canonical `dms` event, which replicates
/// back to us through the feed. On the single-box deployment there is no router and
/// the DM applies directly.
pub async fn send_dm(
    State(st): State<AppState>,
    Path((me, other)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<SendDmReq>,
) -> Res<serde_json::Value> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    match &st.dm_router {
        Some(router) => {
            // Resolve both handles to home-stable ids before leaving this node —
            // the owner authorizes by id and never re-resolves our handles.
            let from = s.find_user(&me).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            let to =
                s.find_user(&other).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            router
                .send_dm(
                    from.id.0,
                    to.id.0,
                    req.sealed_for_recipient.clone(),
                    req.sealed_for_sender.clone(),
                )
                .await
                .map_err(bad)?;
            // The canonical message arrives via the feed; nudge the sender's open
            // conversation to refetch once it lands.
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
        None => {
            let dm = s.social()
                .send_sealed_dm(&me, &other, &req.sealed_for_recipient, &req.sealed_for_sender)
                .map_err(bad)?;
            st.persist();
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "id": dm.id.0 })))
        }
    }
}

/// Permanently block `other`.
///
/// When federated, a block is safety-critical and must silence DMs in both
/// directions — the DM gate runs on the *sender's* home, so the block has to land on
/// **both** users' homes synchronously (via the [`BlockRouter`](app::BlockRouter)),
/// not wait for the eventual feed. On the single-box deployment there is no router
/// and the block applies directly.
pub async fn block(
    State(st): State<AppState>,
    Path((me, other)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    match &st.block_router {
        Some(router) => {
            let blocker =
                s.find_user(&me).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            let blocked =
                s.find_user(&other).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            router.block(blocker.id.0, blocked.id.0).await.map_err(bad)?;
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
        None => {
            s.social().block_user(&me, &other).map_err(bad)?;
            st.persist();
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
    }
}

/// Send a friend request to `other`.
///
/// When federated, a friendship is a two-user record, so the request is committed to
/// **both** users' homes via the [`FriendRouter`](app::FriendRouter) (the addressee's
/// home to show the incoming request, the requester's for the outgoing one). On the
/// single-box deployment it applies directly.
pub async fn request_friend(
    State(st): State<AppState>,
    Path((me, other)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    match &st.friend_router {
        Some(router) => {
            let requester =
                s.find_user(&me).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            let addressee =
                s.find_user(&other).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            router.request(requester.id.0, addressee.id.0).await.map_err(bad)?;
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
        None => {
            s.social().request_friend(&me, &other).map_err(bad)?;
            st.persist();
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
    }
}

/// Accept a pending friend request from `other`.
///
/// When federated, the accepted friendship is committed to **both** users' homes so
/// the friends-only DM gate (which runs on the sender's home) sees it in either
/// direction. On the single-box deployment it applies directly.
pub async fn accept_friend(
    State(st): State<AppState>,
    Path((me, other)): Path<(String, String)>,
    headers: HeaderMap,
) -> Res<serde_json::Value> {
    require_self(&st, &headers, &me)?;
    let s = &st.services;
    match &st.friend_router {
        Some(router) => {
            let accepter =
                s.find_user(&me).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            let requester =
                s.find_user(&other).ok_or((StatusCode::NOT_FOUND, "err.no_such_user".into()))?;
            router.accept(accepter.id.0, requester.id.0).await.map_err(bad)?;
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
        None => {
            s.social().accept_friend(&me, &other).map_err(bad)?;
            st.persist();
            st.publish(social_event(&me, &other));
            Ok(Json(json!({ "ok": true })))
        }
    }
}

/// Set the viewer's DM policy (everyone vs. friends-only).
pub async fn set_policy(
    State(st): State<AppState>,
    Path(me): Path<String>,
    headers: HeaderMap,
    Json(req): Json<DmPolicyReq>,
) -> Res<serde_json::Value> {
    require_self(&st, &headers, &me)?;
    let policy = if req.is_friends_only {
        DmPolicy::FriendsOnly
    } else {
        DmPolicy::Everyone
    };
    st.services.social().set_dm_policy(&me, policy).map_err(bad)?;
    st.persist();
    st.publish(social_event(&me, &me));
    Ok(Json(json!({ "ok": true })))
}
