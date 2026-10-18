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
    /// How to draw their picture (see `avatars::avatar_json`).
    pub avatar: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestView {
    pub handle: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub avatar: serde_json::Value,
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
    // an old username (renamed by staff) still finds its owner
    sqlx::query_as::<_, Person>(
        "SELECT id, handle, display_name FROM users u
          WHERE (lower(u.handle) = lower($1)
                 OR u.id = (SELECT user_id FROM handle_redirects WHERE old_handle = lower($1)))
            AND deleted_at IS NULL AND banned_at IS NULL
          ORDER BY lower(u.handle) = lower($1) DESC LIMIT 1",
    )
    .bind(h)
    .fetch_optional(&st.db)
    .await?
    .ok_or_else(|| {
        AppError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "No Fraglink user with that name.",
        )
    })
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

/// `friend_ids` for public feeds (home, Friends' Activity): test accounts (`users.is_test`)
/// are left out unless `me` is a test account too.
pub async fn public_friend_ids(st: &AppState, me: Uuid) -> ApiResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT u.id FROM friendships f
         JOIN users u ON u.id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
         WHERE (f.user_a = $1 OR f.user_b = $1) AND f.status = 'accepted'
           AND (NOT u.is_test OR (SELECT is_test FROM users WHERE id = $1))",
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

/// `find_by_handle` as seen by `viewer`: a test account (`users.is_test`) only exists for
/// itself and the people it is already friends with; everyone else gets "not found".
pub async fn find_visible(st: &AppState, viewer: Option<Uuid>, handle: &str) -> ApiResult<Person> {
    let p = find_by_handle(st, handle).await?;
    let is_test: bool = sqlx::query_scalar("SELECT is_test FROM users WHERE id = $1")
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
    if !is_test {
        return Ok(p);
    }
    match viewer {
        Some(v) if v == p.id || are_friends(st, v, p.id).await? => Ok(p),
        _ => Err(AppError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "No Fraglink user with that name.",
        )),
    }
}

/// Handles of test accounts, to keep them out of lists other people see.
pub async fn test_handles(st: &AppState) -> ApiResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT handle FROM users WHERE is_test")
        .fetch_all(&st.db)
        .await?)
}

pub async fn request(st: &AppState, me: &Person, handle: &str) -> ApiResult<RequestOutcome> {
    crate::features::require(
        st,
        "friend_requests",
        "Friend requests are turned off right now.",
    )
    .await?;
    let other = find_visible(st, Some(me.id), handle).await?;
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
    maybe_mail_request(st, me, &other).await;
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
    let other = find_visible(st, Some(me.id), handle).await?;
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
        // Declining deletes the request and tells the requester so their "Request sent"
        // row disappears live (app + website sockets).
        sqlx::query("DELETE FROM friendships WHERE user_a = $1 AND user_b = $2")
            .bind(a)
            .bind(b)
            .execute(&st.db)
            .await?;
        st.hub.send(
            other.id,
            ServerMsg::FriendRemoved {
                handle: me.handle.clone(),
            },
        );
        Ok(())
    }
}

/// Unfriend, cancel my request, or decline theirs.
pub async fn remove(st: &AppState, me: &Person, handle: &str) -> ApiResult<()> {
    let other = find_visible(st, Some(me.id), handle).await?;
    let (a, b) = pair(me.id, other.id);
    let was: Option<String> = sqlx::query_scalar(
        "DELETE FROM friendships WHERE user_a = $1 AND user_b = $2 RETURNING status",
    )
    .bind(a)
    .bind(b)
    .fetch_optional(&st.db)
    .await?;
    // Always tell the other side: unfriend, and also cancel-pending (so their
    // incoming request disappears live).
    if was.is_some() {
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
    let other = find_visible(st, Some(me.id), handle).await?;
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
    // To them it looks exactly like being unfriended / the request vanishing.
    if was.is_some() {
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
    let other = find_visible(st, Some(me.id), handle).await?;
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
         WHERE (f.user_a = $1 OR f.user_b = $1) AND u.deleted_at IS NULL AND u.banned_at IS NULL AND u.delete_after IS NULL
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
    let all: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    let avs = crate::avatars::many(st, &all).await?;
    let av = |id: &Uuid| {
        avs.get(id)
            .cloned()
            .unwrap_or_else(|| crate::avatars::avatar_json(*id, None, None, None))
    };
    let mut out = FriendsList::default();
    for (id, handle, display_name, status, by, created, accepted) in rows {
        if status == "accepted" {
            out.friends.push(FriendView {
                presence: pres.get(&id).cloned().unwrap_or_default(),
                handle,
                display_name,
                since: accepted,
                avatar: av(&id),
            });
        } else if by == me {
            out.outgoing.push(RequestView {
                handle,
                display_name,
                created_at: created,
                avatar: av(&id),
            });
        } else {
            out.incoming.push(RequestView {
                handle,
                display_name,
                created_at: created,
                avatar: av(&id),
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

/// Cap on friend-request emails a person can receive in 24 hours.
const FRIEND_REQUEST_MAIL_PER_DAY: i64 = 20;

/// Email the recipient about a new friend request when their preference allows it.
/// Failures are logged only; requesting never waits on mail.
async fn maybe_mail_request(st: &AppState, from: &Person, to: &Person) {
    if let Err(e) = try_mail_request(st, from, to).await {
        tracing::info!(
            from = %from.handle,
            to = %to.handle,
            error = %e.message,
            "friend request email skipped"
        );
    }
}

async fn try_mail_request(st: &AppState, from: &Person, to: &Person) -> ApiResult<()> {
    // Test accounts: only mail when both sides are test (so real users never get
    // mail from hidden test accounts, and we can still exercise the path).
    let (from_test, to_test): (bool, bool) = sqlx::query_as(
        "SELECT (SELECT is_test FROM users WHERE id = $1),
                (SELECT is_test FROM users WHERE id = $2)",
    )
    .bind(from.id)
    .bind(to.id)
    .fetch_one(&st.db)
    .await?;
    if from_test != to_test {
        return Err(AppError::bad_request("test_skip", "test/non-test pair"));
    }

    let row: Option<(String, bool, serde_json::Value)> = sqlx::query_as(
        "SELECT email, email_verified, notify_prefs FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(to.id)
    .fetch_optional(&st.db)
    .await?;
    let Some((email, verified, prefs)) = row else {
        return Ok(());
    };
    if email.trim().is_empty() || !verified {
        return Err(AppError::bad_request("unverified", "no verified email"));
    }
    let want = prefs
        .get("friend_request_email")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !want {
        return Err(AppError::bad_request("pref_off", "preference off"));
    }

    // Already emailed this requester→target once.
    let prior: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM friend_request_mail_log WHERE requester = $1 AND target = $2)",
    )
    .bind(from.id)
    .bind(to.id)
    .fetch_one(&st.db)
    .await?;
    if prior {
        return Err(AppError::bad_request(
            "already_sent",
            "already emailed this pair",
        ));
    }

    let today: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM friend_request_mail_log
         WHERE target = $1 AND sent_at > now() - interval '24 hours'",
    )
    .bind(to.id)
    .fetch_one(&st.db)
    .await?;
    if today >= FRIEND_REQUEST_MAIL_PER_DAY {
        return Err(AppError::bad_request("daily_cap", "daily cap reached"));
    }

    let site = st.cfg.site_url.trim_end_matches('/');
    let friends_url = format!("{site}/friends");
    let off_url = crate::account::notify_off_link(st, to.id, "friend_request_email").await?;
    let e = crate::emails::friend_request(
        &to.display_name,
        &from.display_name,
        &from.handle,
        &friends_url,
        &off_url,
    );
    crate::mailer::send_email(st, &email, &e);
    sqlx::query(
        "INSERT INTO friend_request_mail_log(requester, target) VALUES($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(from.id)
    .bind(to.id)
    .execute(&st.db)
    .await?;
    Ok(())
}
