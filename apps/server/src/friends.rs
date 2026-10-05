//! Friends and blocks (M3). Friend requests go by handle; there is no public
//! user directory. Blocks are one-directional, silent, and remove any friendship.

use crate::error::{ApiResult, AppError};
use crate::presence::{self, ServerMsg};
use crate::AppState;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// Outgoing friend requests allowed per user per hour.
pub const REQUESTS_PER_HOUR: i64 = 30;

fn pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Person {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FriendView {
    pub handle: String,
    pub display_name: String,
    pub since: Option<DateTime<Utc>>,
    pub presence: presence::PresenceView,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestView {
    pub handle: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FriendsList {
    pub friends: Vec<FriendView>,
    pub incoming: Vec<RequestView>,
    pub outgoing: Vec<RequestView>,
    pub blocked: Vec<String>,
}

pub async fn find_by_handle(st: &AppState, handle: &str) -> ApiResult<Person> {
    let h = handle.trim().trim_start_matches('@');
    sqlx::query_as::<_, Person>("SELECT id, handle, display_name FROM users WHERE lower(handle) = lower($1) AND deleted_at IS NULL AND banned_at IS NULL")
        .bind(h)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "user_not_found", "No Fraglink user with that name."))
}

async fn blocked_either(st: &AppState, a: Uuid, b: Uuid) -> ApiResult<(bool, bool)> {
    let (ab, ba): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM blocks WHERE blocker = $1 AND blocked = $2),
                EXISTS(SELECT 1 FROM blocks WHERE blocker = $2 AND blocked = $1)",
    )
    .bind(a)
    .bind(b)
    .fetch_one(&st.db)
    .await?;
    Ok((ab, ba))
}

/// Accepted friends' ids (blocks always remove friendships, so no extra filter needed).
pub async fn friend_ids(st: &AppState, me: Uuid) -> ApiResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END FROM friendships
         WHERE (user_a = $1 OR user_b = $1) AND status = 'accepted'",
    )
    .bind(me)
    .fetch_all(&st.db)
    .await?)
}

pub async fn are_friends(st: &AppState, a: Uuid, b: Uuid) -> ApiResult<bool> {
    let (x, y) = pair(a, b);
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM friendships WHERE user_a = $1 AND user_b = $2 AND status = 'accepted')")
        .bind(x)
        .bind(y)
        .fetch_one(&st.db)
        .await?)
}

/// Result of a friend request, as shown to the requester.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RequestOutcome {
    Pending,
    Accepted,
}

pub async fn request(st: &AppState, me: &Person, handle: &str) -> ApiResult<RequestOutcome> {
    let other = find_by_handle(st, handle).await?;
    if other.id == me.id {
        return Err(AppError::bad_request(
            "self_request",
            "You can't add yourself.",
        ));
    }
    let (i_block, they_block) = blocked_either(st, me.id, other.id).await?;
    if i_block {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "you_blocked",
            "You blocked this user. Unblock them first.",
        ));
    }
    let (a, b) = pair(me.id, other.id);
    let existing: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT status, requested_by FROM friendships WHERE user_a = $1 AND user_b = $2",
    )
    .bind(a)
    .bind(b)
    .fetch_optional(&st.db)
    .await?;
    match existing {
        Some((s, _)) if s == "accepted" => return Ok(RequestOutcome::Accepted),
        Some((_, by)) if by == me.id => return Ok(RequestOutcome::Pending),
        Some(_) => {
            // They already asked me: asking back accepts.
            accept(st, me, &other).await?;
            return Ok(RequestOutcome::Accepted);
        }
        None => {}
    }
    // Being blocked is invisible to the requester: same answer, nothing stored.
    if they_block {
        return Ok(RequestOutcome::Pending);
    }
    let recent: i64 = sqlx::query_scalar("SELECT count(*) FROM friendships WHERE requested_by = $1 AND created_at > now() - interval '1 hour'")
        .bind(me.id)
        .fetch_one(&st.db)
        .await?;
    if recent >= REQUESTS_PER_HOUR {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_requests",
            "Too many friend requests. Try again later.",
        ));
    }
    sqlx::query("INSERT INTO friendships(user_a, user_b, status, requested_by) VALUES($1, $2, 'pending', $3) ON CONFLICT DO NOTHING")
        .bind(a)
        .bind(b)
        .bind(me.id)
        .execute(&st.db)
        .await?;
    st.hub.send(
        other.id,
        ServerMsg::FriendRequest {
            handle: me.handle.clone(),
            display_name: me.display_name.clone(),
        },
    );
    Ok(RequestOutcome::Pending)
}

async fn accept(st: &AppState, me: &Person, other: &Person) -> ApiResult<()> {
    let (a, b) = pair(me.id, other.id);
    sqlx::query("UPDATE friendships SET status = 'accepted', accepted_at = now() WHERE user_a = $1 AND user_b = $2")
        .bind(a)
        .bind(b)
        .execute(&st.db)
        .await?;
    // Tell both sides and exchange current presence.
    st.hub.send(
        other.id,
        ServerMsg::FriendAccepted {
            handle: me.handle.clone(),
        },
    );
    st.hub.send(
        me.id,
        ServerMsg::FriendAccepted {
            handle: other.handle.clone(),
        },
    );
    if let Some(p) = presence::visible_presence(st, me.id).await? {
        st.hub.send(
            other.id,
            ServerMsg::Presence(p.with_handle(&me.handle, false)),
        );
    }
    if let Some(p) = presence::visible_presence(st, other.id).await? {
        st.hub.send(
            me.id,
            ServerMsg::Presence(p.with_handle(&other.handle, false)),
        );
    }
    Ok(())
}

/// Accept or decline a request the other user sent me.
pub async fn respond(st: &AppState, me: &Person, handle: &str, accept_it: bool) -> ApiResult<()> {
    let other = find_by_handle(st, handle).await?;
    let (a, b) = pair(me.id, other.id);
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM friendships WHERE user_a = $1 AND user_b = $2 AND status = 'pending' AND requested_by = $3)",
    )
    .bind(a)
    .bind(b)
    .bind(other.id)
    .fetch_one(&st.db)
    .await?;
    if !pending {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "no_request",
            "There is no request from that user.",
        ));
    }
    if accept_it {
        accept(st, me, &other).await
    } else {
        // Declining deletes the request without notifying the requester.
        sqlx::query("DELETE FROM friendships WHERE user_a = $1 AND user_b = $2")
            .bind(a)
            .bind(b)
            .execute(&st.db)
            .await?;
        Ok(())
    }
}

/// Unfriend, cancel my request, or decline theirs.
pub async fn remove(st: &AppState, me: &Person, handle: &str) -> ApiResult<()> {
    let other = find_by_handle(st, handle).await?;
    let (a, b) = pair(me.id, other.id);
    let was: Option<String> = sqlx::query_scalar(
        "DELETE FROM friendships WHERE user_a = $1 AND user_b = $2 RETURNING status",
    )
    .bind(a)
    .bind(b)
    .fetch_optional(&st.db)
    .await?;
    if was.as_deref() == Some("accepted") {
        st.hub.send(
            other.id,
            ServerMsg::FriendRemoved {
                handle: me.handle.clone(),
            },
        );
    }
    st.hub.send(
        me.id,
        ServerMsg::FriendRemoved {
            handle: other.handle.clone(),
        },
    );
    Ok(())
}

pub async fn block(st: &AppState, me: &Person, handle: &str) -> ApiResult<()> {
    let other = find_by_handle(st, handle).await?;
    if other.id == me.id {
        return Err(AppError::bad_request(
            "self_block",
            "You can't block yourself.",
        ));
    }
    let (a, b) = pair(me.id, other.id);
    let mut tx = st.db.begin().await?;
    let was: Option<String> = sqlx::query_scalar(
        "DELETE FROM friendships WHERE user_a = $1 AND user_b = $2 RETURNING status",
    )
    .bind(a)
    .bind(b)
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO blocks(blocker, blocked) VALUES($1, $2) ON CONFLICT DO NOTHING")
        .bind(me.id)
        .bind(other.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // To them it looks exactly like being unfriended.
    if was.as_deref() == Some("accepted") {
        st.hub.send(
            other.id,
            ServerMsg::FriendRemoved {
                handle: me.handle.clone(),
            },
        );
    }
    st.hub.send(
        me.id,
        ServerMsg::FriendRemoved {
            handle: other.handle.clone(),
        },
    );
    Ok(())
}

pub async fn unblock(st: &AppState, me: &Person, handle: &str) -> ApiResult<()> {
    let other = find_by_handle(st, handle).await?;
    sqlx::query("DELETE FROM blocks WHERE blocker = $1 AND blocked = $2")
        .bind(me.id)
        .bind(other.id)
        .execute(&st.db)
        .await?;
    Ok(())
}

pub async fn list(st: &AppState, me: Uuid) -> ApiResult<FriendsList> {
    type Row = (
        Uuid,
        String,
        String,
        String,
        Uuid,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT u.id, u.handle, u.display_name, f.status, f.requested_by, f.created_at, f.accepted_at
         FROM friendships f JOIN users u ON u.id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
         WHERE (f.user_a = $1 OR f.user_b = $1) AND u.deleted_at IS NULL AND u.banned_at IS NULL
         ORDER BY lower(u.handle)",
    )
    .bind(me)
    .fetch_all(&st.db)
    .await?;
    let ids: Vec<Uuid> = rows
        .iter()
        .filter(|r| r.3 == "accepted")
        .map(|r| r.0)
        .collect();
    let pres = presence::visible_presence_many(st, &ids).await?;
    let mut out = FriendsList::default();
    for (id, handle, display_name, status, by, created, accepted) in rows {
        if status == "accepted" {
            out.friends.push(FriendView {
                presence: pres.get(&id).cloned().unwrap_or_default(),
                handle,
                display_name,
                since: accepted,
            });
        } else if by == me {
            out.outgoing.push(RequestView {
                handle,
                display_name,
                created_at: created,
            });
        } else {
            out.incoming.push(RequestView {
                handle,
                display_name,
                created_at: created,
            });
        }
    }
    out.blocked = sqlx::query_scalar("SELECT u.handle FROM blocks b JOIN users u ON u.id = b.blocked WHERE b.blocker = $1 ORDER BY lower(u.handle)")
        .bind(me)
        .fetch_all(&st.db)
        .await?;
    Ok(out)
}

pub async fn export(st: &AppState, me: Uuid) -> ApiResult<Value> {
    let l = list(st, me).await?;
    Ok(json!({
        "friends": l.friends.iter().map(|f| json!({ "handle": f.handle, "display_name": f.display_name, "since": f.since })).collect::<Vec<_>>(),
        "incoming_requests": l.incoming,
        "outgoing_requests": l.outgoing,
        "blocked": l.blocked,
    }))
}
