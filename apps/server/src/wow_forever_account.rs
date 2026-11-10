//! Forever account ownership is separate from Retail rename/merge and /played.
//! Only this module consumes the realmless Forever user-token account listing.
use crate::{wow, AppState};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

const ACCOUNT_URL: &str =
    "https://us.api.blizzard.com/profile/user/wow?namespace=profile-forever-us&locale=en_US";

#[derive(Debug, Clone, sqlx::FromRow)]
struct AccountCharacter {
    name: String,
    bnet_id: i64,
    addon_guid: Option<String>,
    level: Option<i32>,
    class_name: Option<String>,
    faction: Option<String>,
}

/// Fail closed on unknown/partial schemas. A 404 is not a confirmed empty account.
/// `guid` is optional and matched literally; never infer a Blizzard id from UnitGUID.
fn parse(v: &Value) -> Option<Vec<AccountCharacter>> {
    let mut out: Vec<AccountCharacter> = Vec::new();
    for account in v["wow_accounts"].as_array()? {
        for c in account["characters"].as_array()? {
            let first = c["name"].as_str()?.trim();
            let surname = c["surname"].as_str()?.trim();
            let id = c["id"].as_i64().filter(|id| *id > 0)?;
            if first.is_empty() || surname.is_empty() {
                return None;
            }
            let row = AccountCharacter {
                name: format!("{first} {surname}"),
                bnet_id: id,
                addon_guid: c["guid"]
                    .as_str()
                    .filter(|g| !g.is_empty())
                    .map(str::to_string),
                level: c["level"]
                    .as_i64()
                    .and_then(|l| i32::try_from(l).ok())
                    .filter(|l| *l > 0),
                class_name: wow::name_of(&c["playable_class"])
                    .map(|n| {
                        n.chars()
                            .filter(|c| c.is_ascii_alphabetic())
                            .collect::<String>()
                            .to_uppercase()
                    })
                    .filter(|n| !n.is_empty()),
                faction: wow::profile_faction(c).map(str::to_string),
            };
            if out
                .iter()
                .any(|r| r.bnet_id == id || r.name.eq_ignore_ascii_case(&row.name))
            {
                return None; // ambiguity is not permission to pick a character
            }
            out.push(row);
        }
    }
    Some(out)
}

async fn fetch(token: &str) -> Option<Vec<AccountCharacter>> {
    let resp = wow::http()?
        .get(ACCOUNT_URL)
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    // Do not log response bodies or tokens.
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "Forever account listing did not answer successfully");
        return None;
    }
    let v = resp.json::<Value>().await.ok()?;
    let rows = parse(&v);
    if rows.is_none() {
        tracing::warn!("Forever account listing schema not recognised; cache left alone");
    }
    rows
}

/// The optional probe runs the same GET/parser during OAuth but performs no DB
/// work. Both modes default off. Tokens are used here then discarded by OAuth.
pub(crate) async fn sync(st: &AppState, uid: Uuid, token: &str) -> Option<usize> {
    let enabled = wow::forever_account_enabled();
    let probe = matches!(
        std::env::var("BNET_FOREVER_ACCOUNT_PROBE").ok().as_deref(),
        Some("on" | "1")
    ) && std::env::var("BNET_FOREVER_ACCOUNT_PROBE_USER")
        .ok()
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        == Some(uid);
    if !enabled && !probe {
        return None;
    }
    let Some(rows) = fetch(token).await else {
        return None;
    };
    match apply(&st.db, uid, &rows, enabled).await {
        Ok(true) => Some(rows.len()),
        Ok(false) => None,
        Err(e) => {
            tracing::warn!("Forever account cache for {uid}: {e}");
            None
        }
    }
}

async fn apply(
    db: &PgPool,
    uid: Uuid,
    rows: &[AccountCharacter],
    enabled: bool,
) -> Result<bool, sqlx::Error> {
    if !enabled {
        for c in rows {
            tracing::info!(%uid, name = %c.name, bnet_id = c.bnet_id, level = ?c.level,
                "Forever account read-only probe character");
        }
        tracing::info!(
            count = rows.len(),
            "Forever account read-only probe parsed successfully; no Forever writes"
        );
        return Ok(false);
    }
    replace_cache(db, uid, rows).await?;
    Ok(true)
}

async fn replace_cache(
    db: &PgPool,
    uid: Uuid,
    rows: &[AccountCharacter],
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "DELETE FROM bnet_characters WHERE user_id=$1 AND game_id='wow_forever' AND region='us'",
    )
    .bind(uid)
    .execute(&mut *tx)
    .await?;
    for c in rows {
        sqlx::query("INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, realm_name, name, bnet_id, addon_guid, level, class_name, faction)
            VALUES($1, 'wow_forever', 'us', '', '', $2, $3, $4, $5, $6, $7)")
            .bind(uid).bind(&c.name).bind(c.bnet_id).bind(&c.addon_guid).bind(c.level)
            .bind(&c.class_name).bind(&c.faction).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    verify_cached(db, uid).await
}

#[derive(sqlx::FromRow)]
struct LocalCharacter {
    id: Uuid,
    name: String,
    bnet_id: Option<i64>,
    addon_guid: Option<String>,
    battletag: String,
}

fn matches(c: &LocalCharacter, a: &AccountCharacter) -> bool {
    // Known ids/GUIDs must not conflict, even when the display names match.
    if c.bnet_id.is_some_and(|id| id != a.bnet_id)
        || matches!((&c.addon_guid, &a.addon_guid), (Some(x), Some(y)) if x != y)
    {
        return false;
    }
    c.bnet_id == Some(a.bnet_id)
        || matches!((&c.addon_guid, &a.addon_guid), (Some(x), Some(y)) if x == y)
        || c.name.eq_ignore_ascii_case(&a.name)
}

/// No inserts/deletes/renames on wow_characters, no quarantine replay, no totals.
/// Only existing realmless US rows can be verified. Ambiguous matches stay put.
pub(crate) async fn verify_cached(db: &PgPool, uid: Uuid) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    let tag: Option<String> = sqlx::query_scalar(
        "SELECT display FROM linked_accounts WHERE user_id=$1 AND provider='bnet' AND verified",
    )
    .bind(uid)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    let mut accounts: Vec<AccountCharacter> = sqlx::query_as("SELECT name, bnet_id, addon_guid, level, class_name, faction FROM bnet_characters
        WHERE user_id=$1 AND game_id='wow_forever' AND region='us' AND realm_slug='' AND bnet_id IS NOT NULL")
        .bind(uid).fetch_all(&mut *tx).await?;
    accounts.retain(|a| a.bnet_id > 0 && a.name.split_whitespace().count() >= 2);
    let locals: Vec<LocalCharacter> = sqlx::query_as("SELECT id, name, bnet_id, addon_guid, battletag FROM wow_characters
        WHERE user_id=$1 AND game_id='wow_forever' AND region='us' AND realm='' AND realm_slug='' FOR UPDATE")
        .bind(uid).fetch_all(&mut *tx).await?;
    for c in &locals {
        let candidates: Vec<_> = accounts.iter().filter(|a| matches(c, a)).collect();
        let a = candidates.first().copied().filter(|a| {
            candidates.len() == 1
                && locals.iter().filter(|other| matches(other, a)).count() == 1
                && tag.as_ref().is_some_and(|t| {
                    !t.is_empty() && !c.battletag.is_empty() && t.eq_ignore_ascii_case(&c.battletag)
                })
        });
        let mut verified = false;
        if let Some(a) = a {
            let newer_owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bnet_characters o
                WHERE o.user_id<>$1 AND o.game_id='wow_forever' AND o.region='us' AND o.bnet_id=$2
                  AND o.fetched_at > coalesce((SELECT max(m.fetched_at) FROM bnet_characters m
                    WHERE m.user_id=$1 AND m.game_id='wow_forever' AND m.region='us' AND m.bnet_id=$2), '-infinity'))")
                .bind(uid).bind(a.bnet_id).fetch_one(&mut *tx).await?;
            verified = !newer_owner;
            if verified {
                sqlx::query(
                    "UPDATE wow_characters SET verified=true, ever_verified=true,
                    bnet_id=coalesce(bnet_id, $2), bnet_level=coalesce($3, bnet_level),
                    class=coalesce($4, class), faction=coalesce($5, faction) WHERE id=$1",
                )
                .bind(c.id)
                .bind(a.bnet_id)
                .bind(a.level)
                .bind(&a.class_name)
                .bind(&a.faction)
                .execute(&mut *tx)
                .await?;
            }
        }
        if !verified {
            sqlx::query("UPDATE wow_characters SET verified=false WHERE id=$1")
                .bind(c.id)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> Value {
        json!({"wow_accounts": [{"characters": [{"id": 111, "name": "Vale", "surname": "Ashborne", "level": 18,
            "playable_class": {"name": "Warrior"}, "faction": {"type": "HORDE"}}]}]})
    }

    #[test]
    fn full_name_and_metadata_from_separate_surname() {
        let rows = parse(&payload()).unwrap();
        assert_eq!(rows[0].name, "Vale Ashborne");
        assert_eq!(rows[0].level, Some(18));
        assert_eq!(rows[0].class_name.as_deref(), Some("WARRIOR"));
        assert_eq!(rows[0].faction.as_deref(), Some("HORDE"));
        assert_eq!(ACCOUNT_URL, "https://us.api.blizzard.com/profile/user/wow?namespace=profile-forever-us&locale=en_US");
    }

    #[test]
    fn partial_or_ambiguous_payload_is_not_an_empty_account() {
        assert!(parse(&json!({})).is_none());
        assert!(parse(&json!({"wow_accounts": [{}]})).is_none());
        for bad in [Value::Null, json!(""), json!(42)] {
            let mut v = payload();
            v["wow_accounts"][0]["characters"][0]["surname"] = bad;
            assert!(parse(&v).is_none());
        }
        let mut v = payload();
        let dup = v["wow_accounts"][0]["characters"][0].clone();
        v["wow_accounts"][0]["characters"]
            .as_array_mut()
            .unwrap()
            .push(dup);
        assert!(parse(&v).is_none());
        assert!(parse(&json!({"wow_accounts": []})).unwrap().is_empty());
    }

    #[test]
    fn stable_id_guid_and_full_name_match_without_inference() {
        let mut a = parse(&payload()).unwrap().remove(0);
        let mut c = LocalCharacter {
            id: Uuid::new_v4(),
            name: "VALE ASHBORNE".into(),
            bnet_id: None,
            addon_guid: None,
            battletag: "Vale#1".into(),
        };
        assert!(matches(&c, &a));
        c.name = "Old Name".into();
        assert!(!matches(&c, &a));
        c.bnet_id = Some(111);
        assert!(matches(&c, &a));
        c.bnet_id = None;
        c.addon_guid = Some("Player-123-ABC".into());
        a.addon_guid = c.addon_guid.clone();
        assert!(matches(&c, &a));
        c.bnet_id = Some(222);
        assert!(!matches(&c, &a));
        c.bnet_id = Some(111);
        a.addon_guid = Some("Player-123-DEF".into());
        assert!(!matches(&c, &a));
    }

    async fn fixture(db: &PgPool) -> Uuid {
        let uid = Uuid::new_v4();
        sqlx::query("INSERT INTO games(id,name) VALUES('wow_forever','World of Warcraft Forever') ON CONFLICT DO NOTHING").execute(db).await.unwrap();
        sqlx::query("INSERT INTO users(id,email,handle,display_name,password_hash) VALUES($1,'forever@example.com','forevertest','Forever Test','unused')")
            .bind(uid).execute(db).await.unwrap();
        sqlx::query("INSERT INTO linked_accounts(user_id,provider,provider_uid,display,verified,scopes) VALUES($1,'bnet','test-forever','Vale#1',true,'openid wow.profile')")
            .bind(uid).execute(db).await.unwrap();
        uid
    }

    async fn character(
        db: &PgPool,
        uid: Uuid,
        name: &str,
        bid: Option<i64>,
        guid: Option<&str>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO wow_characters(id,user_id,game_id,region,realm,realm_slug,name,played_s,played_at,battletag,bnet_id,addon_guid,hidden)
            VALUES($1,$2,'wow_forever','us','','',$3,9876,now(),'Vale#1',$4,$5,true)")
            .bind(id).bind(uid).bind(name).bind(bid).bind(guid).execute(db).await.unwrap();
        sqlx::query("INSERT INTO wow_played_snaps(char_id,played_s,at) VALUES($1,1234,now()-interval '1 day'),($1,9876,now())")
            .bind(id).execute(db).await.unwrap();
        id
    }

    // Everything except ownership and the explicitly allowed profile fields,
    // plus every hours/history table. IDs, names, timestamps, GUIDs included.
    async fn protected(db: &PgPool) -> Value {
        let mut out = serde_json::Map::new();
        let chars: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(c) - ARRAY['verified','ever_verified','bnet_id','bnet_level','class','faction'] ORDER BY id),'[]') FROM wow_characters c")
            .fetch_one(db).await.unwrap();
        out.insert("characters".into(), chars);
        for (table, query) in [
            ("wow_played_snaps", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM wow_played_snaps t) s"),
            ("wow_character_names", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM wow_character_names t) s"),
            ("wow_character_merges", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM wow_character_merges t) s"),
            ("wow_played_quarantine", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM wow_played_quarantine t) s"),
            ("wow_flags", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM wow_flags t) s"),
            ("sessions", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM sessions t) s"),
            ("hours_entries", "SELECT coalesce(jsonb_agg(v ORDER BY v::text), '[]') FROM (SELECT to_jsonb(t) v FROM hours_entries t) s"),
        ] {
            let v: Value = sqlx::query_scalar(query).fetch_one(db).await.unwrap();
            out.insert(table.into(), v);
        }
        Value::Object(out)
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn verification_preserves_ids_hours_history_and_never_merges(db: PgPool) {
        let uid = fixture(&db).await;
        let full = character(&db, uid, "Vale Ashborne", None, None).await;
        let id_match = character(&db, uid, "Old Surname", Some(222), None).await;
        let guid_match = character(&db, uid, "Other Oldname", None, Some("Player-Test-ABC")).await;
        sqlx::query("INSERT INTO hours_entries(id,user_id,game_id,tier,source,minutes,inputs) VALUES($1,$2,'wow_forever','imported','wow_addon',164,jsonb_build_object('seconds',9876))")
            .bind(Uuid::new_v4()).bind(uid).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO wow_played_quarantine(id,user_id,claimed_game,reason,detail,payload) VALUES($1,$2,'wow_forever','test','untouched','{}')")
            .bind(Uuid::new_v4()).bind(uid).execute(&db).await.unwrap();
        let before = protected(&db).await;
        let mut rows = parse(&payload()).unwrap();
        let mut by_id = rows[0].clone();
        by_id.bnet_id = 222;
        by_id.name = "New Surname".into();
        rows.push(by_id);
        let mut by_guid = rows[0].clone();
        by_guid.bnet_id = 333;
        by_guid.name = "New Othername".into();
        by_guid.addon_guid = Some("Player-Test-ABC".into());
        rows.push(by_guid);
        replace_cache(&db, uid, &rows).await.unwrap();
        verify_cached(&db, uid).await.unwrap();
        assert_eq!(protected(&db).await, before);
        let verified: Vec<(Uuid, bool, bool, Option<i64>, Option<i32>, String, Option<String>)> = sqlx::query_as("SELECT id,verified,ever_verified,bnet_id,bnet_level,class,faction FROM wow_characters ORDER BY id")
            .fetch_all(&db).await.unwrap();
        assert_eq!(verified.len(), 3);
        for (id, v, ever, bid, level, class, faction) in verified {
            assert!(v && ever);
            assert_eq!(
                bid,
                Some(if id == full {
                    111
                } else if id == id_match {
                    222
                } else {
                    assert_eq!(id, guid_match);
                    333
                })
            );
            assert_eq!(level, Some(18));
            assert_eq!(class, "WARRIOR");
            assert_eq!(faction.as_deref(), Some("HORDE"));
        }
        // A valid empty account revokes ticks without touching protected fields.
        replace_cache(&db, uid, &[]).await.unwrap();
        assert_eq!(protected(&db).await, before);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE verified")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn probe_cannot_write_cache_or_character_fields(db: PgPool) {
        let uid = fixture(&db).await;
        character(&db, uid, "Vale Ashborne", None, None).await;
        let before = protected(&db).await;
        let rows = parse(&payload()).unwrap();
        assert!(!apply(&db, uid, &rows, false).await.unwrap());
        assert_eq!(protected(&db).await, before);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM bnet_characters")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE verified OR ever_verified OR bnet_id IS NOT NULL").fetch_one(&db).await.unwrap();
        assert_eq!(count, 0);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn ambiguous_conflicting_and_wrong_tag_rows_are_not_verified(db: PgPool) {
        let uid = fixture(&db).await;
        character(&db, uid, "Vale Ashborne", None, None).await;
        character(&db, uid, "Old Name", Some(111), None).await;
        let mut rows = parse(&payload()).unwrap();
        replace_cache(&db, uid, &rows).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE verified")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0, "two locals for one account id are not combined");
        sqlx::query("UPDATE wow_characters SET bnet_id=999 WHERE name='Old Name'")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE wow_characters SET battletag='Other#1' WHERE name='Vale Ashborne'")
            .execute(&db)
            .await
            .unwrap();
        rows[0].addon_guid = Some("Player-Test-ABC".into());
        replace_cache(&db, uid, &rows).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE verified")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
