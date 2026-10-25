// handlers/weekly_leaderboard.rs
//
// Weekly leaderboard driven by two fields on the `users` document:
//   weekly_points: i32   points WON this week (wins only, never negative)
//   week_key:      String week id, e.g. "2026-W43"; weeks run Tue 04:00 EAT -> next Tue 04:00 EAT
//
// `season_points` stays the spendable balance and is not touched here.
// Users whose week_key is stale are simply filtered out, so no reset cron.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use bson::{doc, oid::ObjectId, Bson, Document};
use chrono::{Datelike, FixedOffset, TimeZone, Utc};
use futures_util::TryStreamExt;
use mongodb::{ClientSession, Collection, Database};
use serde::Deserialize;
use serde_json::json;

use crate::errors::{AppError, Result};
use crate::models::channel::Channel;
use crate::AppState;

const BOARD_SIZE: i64 = 50;

/// The week runs Tuesday 04:00 EAT -> next Tuesday 04:00 EAT.
/// Trick: shift "now" back 28h (1 day + 4h) so the boundary lands on
/// Monday 00:00, then use the normal ISO week of the shifted time.
const WEEK_SHIFT_HOURS: i64 = 28;

fn eat() -> FixedOffset {
    FixedOffset::east_opt(3 * 3600).unwrap()
}

/// e.g. "2026-W43". Changes exactly at Tuesday 04:00 EAT.
pub fn current_week_key() -> String {
    let shifted = Utc::now().with_timezone(&eat()) - chrono::Duration::hours(WEEK_SHIFT_HOURS);
    let w = shifted.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

/// (start, end) of the current week as RFC3339 strings in EAT, for countdowns.
pub fn current_week_bounds() -> (String, String) {
    let tz = eat();
    let shifted = Utc::now().with_timezone(&tz) - chrono::Duration::hours(WEEK_SHIFT_HOURS);
    let monday = shifted.date_naive()
        - chrono::Duration::days(shifted.weekday().num_days_from_monday() as i64);
    let start_naive = monday.and_hms_opt(0, 0, 0).unwrap()
        + chrono::Duration::hours(WEEK_SHIFT_HOURS);
    let start = tz.from_local_datetime(&start_naive).unwrap();
    let end = start + chrono::Duration::days(7);
    (start.to_rfc3339(), end.to_rfc3339())
}

/// Add `delta` winnings to the user's weekly score. Rolls the week over
/// atomically in ONE update (pipeline update), so there is no race between
/// "reset" and "add". No-op when delta <= 0: only wins count.
///
/// Pass `Some(&mut session)` when called inside a transaction.
pub async fn bump_weekly(
    db: &Database,
    user_id: &str,
    delta: i32,
    session: Option<&mut ClientSession>,
) -> Result<()> {
    if delta <= 0 {
        return Ok(());
    }
    let oid = ObjectId::parse_str(user_id)?;
    let wk = current_week_key();
    let users: Collection<Document> = db.collection("users");

    let pipeline = vec![doc! { "$set": {
        "weekly_points": { "$add": [
            { "$cond": [
                { "$eq": ["$week_key", &wk] },
                { "$ifNull": ["$weekly_points", 0] },
                0
            ]},
            delta
        ]},
        "week_key": &wk,
    }}];

    // mongodb 3.4: `.session()` takes `&mut ClientSession`, not an Option.
    let update = users.update_one(doc! { "_id": oid }, pipeline);
    match session {
        Some(s) => update.session(s).await?,
        None => update.await?,
    };
    Ok(())
}

fn read_i64(d: &Document, key: &str) -> i64 {
    match d.get(key) {
        Some(Bson::Int32(v)) => *v as i64,
        Some(Bson::Int64(v)) => *v,
        Some(Bson::Double(v)) => *v as i64,
        _ => 0,
    }
}

async fn build_board(
    db: &Database,
    member_oids: Option<Vec<ObjectId>>,
    me: Option<&str>,
) -> Result<serde_json::Value> {
    let users: Collection<Document> = db.collection("users");
    let wk = current_week_key();

    let mut filter = doc! { "week_key": &wk, "weekly_points": { "$gt": 0 } };
    if let Some(ids) = &member_oids {
        filter.insert("_id", doc! { "$in": ids });
    }

    let rows: Vec<Document> = users
        .find(filter.clone())
        .sort(doc! { "weekly_points": -1, "_id": 1 })
        .limit(BOARD_SIZE)
        .projection(doc! { "username": 1, "weekly_points": 1 })
        .await?
        .try_collect()
        .await?;

    let board: Vec<serde_json::Value> = rows
        .iter()
        .enumerate()
        .map(|(i, d)| {
            json!({
                "rank": i + 1,
                "user_id": d.get_object_id("_id").map(|o| o.to_hex()).unwrap_or_default(),
                "username": d.get_str("username").unwrap_or(""),
                "weekly_points": read_i64(d, "weekly_points"),
            })
        })
        .collect();

    // The caller's own rank, even if they're outside the top 50.
    let mut me_json = serde_json::Value::Null;
    if let Some(uid) = me {
        if let Ok(oid) = ObjectId::parse_str(uid) {
            let in_scope = member_oids.as_ref().map_or(true, |ids| ids.contains(&oid));
            if in_scope {
                if let Some(u) = users.find_one(doc! { "_id": oid }).await? {
                    let this_week = u.get_str("week_key").map_or(false, |k| k == wk);
                    let pts = if this_week { read_i64(&u, "weekly_points") } else { 0 };
                    let rank = if pts > 0 {
                        let mut gt = filter.clone();
                        gt.insert("weekly_points", doc! { "$gt": pts });
                        Some(users.count_documents(gt).await? + 1)
                    } else {
                        None
                    };
                    me_json = json!({
                        "user_id": uid,
                        "username": u.get_str("username").unwrap_or(""),
                        "weekly_points": pts,
                        "rank": rank,
                    });
                }
            }
        }
    }

    let (week_starts_at, week_ends_at) = current_week_bounds();
    Ok(json!({
        "success": true,
        "week": wk,
        "week_starts_at": week_starts_at,
        "week_ends_at": week_ends_at,
        "leaderboard": board,
        "count": board.len(),
        "me": me_json,
    }))
}

#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    pub user_id: Option<String>,
}

// GET /api/leaderboard/weekly?user_id=...
pub async fn get_global_weekly_leaderboard_handler(
    State(state): State<AppState>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(build_board(&state.db, None, q.user_id.as_deref()).await?))
}

// GET /api/channels/:channel_id/leaderboard/weekly?user_id=...
pub async fn get_channel_weekly_leaderboard_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(q): Query<BoardQuery>,
) -> Result<Json<serde_json::Value>> {
    let channels: Collection<Channel> = state.db.collection("channels");
    let channel = channels
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let oids: Vec<ObjectId> = channel
        .members
        .iter()
        .filter_map(|m| ObjectId::parse_str(&m.user_id).ok())
        .collect();

    let mut v = build_board(&state.db, Some(oids), q.user_id.as_deref()).await?;
    v["channel_id"] = json!(channel_id);
    v["channel_name"] = json!(channel.name);
    Ok(Json(v))
}