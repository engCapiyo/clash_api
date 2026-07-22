use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime, Document};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tracing;

use crate::errors::{AppError, Result};
use crate::handlers::ws_handler::broadcast_live_match_update;
use crate::models::channel::Vote;
use crate::models::game::{
    Coach, CommentaryEntry, Game, GameQuery, HistoryGame, LineupsDocument, LiveGameUpdate,
    MatchStatistics, Player, StatisticsSnapshot, TeamLineup, TeamStatistics, Voter,
};
use crate::models::notification::FCMToken;
use crate::state::AppState;

// ============================================================================
// COLLECTION NAMES
// ============================================================================

const GAMES_COLLECTION: &str = "games";
const FIXTURES_COLLECTION: &str = "fixtures";

// ============================================================================
// TOLERANT GAME FETCHING
// ============================================================================

async fn find_game_tolerant(
    collection: &Collection<Game>,
    filter: Document,
) -> Result<Option<Game>> {
    let raw_collection: Collection<Document> = collection.clone_with_type();

    let raw_doc = match raw_collection.find_one(filter).await? {
        Some(d) => d,
        None => return Ok(None),
    };

    match mongodb::bson::from_document::<Game>(raw_doc.clone()) {
        Ok(game) => Ok(Some(game)),
        Err(e) => {
            let match_id = raw_doc
                .get_str("matchId")
                .unwrap_or("<unknown>")
                .to_string();
            tracing::warn!(
                "⚠️ Game document {} failed to deserialize ({}); retrying with lineups stripped",
                match_id,
                e
            );

            let mut stripped = raw_doc;
            stripped.remove("lineups");

            match mongodb::bson::from_document::<Game>(stripped) {
                Ok(game) => {
                    tracing::warn!(
                        "⚠️ Recovered fixture {} by dropping malformed 'lineups' field",
                        match_id
                    );
                    Ok(Some(game))
                }
                Err(e2) => {
                    tracing::error!(
                        "❌ Fixture {} still fails to deserialize after stripping lineups: {}",
                        match_id,
                        e2
                    );
                    Err(AppError::InternalServerError(format!(
                        "Malformed fixture document {}: {}",
                        match_id, e2
                    )))
                }
            }
        }
    }
}

async fn find_games_tolerant(collection: &Collection<Game>, filter: Document) -> Result<Vec<Game>> {
    let raw_collection: Collection<Document> = collection.clone_with_type();
    let mut cursor = raw_collection.find(filter).await?;

    let mut games: Vec<Game> = Vec::new();
    let mut skipped = 0;

    while cursor.advance().await? {
        let raw_doc = cursor.deserialize_current()?;
        match mongodb::bson::from_document::<Game>(raw_doc.clone()) {
            Ok(game) => games.push(game),
            Err(e) => {
                let match_id = raw_doc
                    .get_str("matchId")
                    .unwrap_or("<unknown>")
                    .to_string();
                let mut stripped = raw_doc;
                stripped.remove("lineups");
                match mongodb::bson::from_document::<Game>(stripped) {
                    Ok(game) => {
                        tracing::warn!(
                            "⚠️ Recovered fixture {} by dropping malformed 'lineups' field",
                            match_id
                        );
                        games.push(game);
                    }
                    Err(e2) => {
                        skipped += 1;
                        tracing::warn!(
                            "⚠️ Skipping malformed document {}: {} (original: {})",
                            match_id,
                            e2,
                            e
                        );
                    }
                }
            }
        }
    }

    if skipped > 0 {
        tracing::warn!("⚠️ Skipped {} malformed documents", skipped);
    }

    Ok(games)
}

// ============================================================================
// TOLERANT HISTORY GAME FETCHING
// ============================================================================

async fn find_history_games_tolerant(
    collection: &Collection<HistoryGame>,
    filter: Document,
    sort: Document,
    skip: u64,
    limit: i64,
) -> Result<(Vec<HistoryGame>, i64)> {
    let raw_collection: Collection<Document> = collection.clone_with_type();
    let mut cursor = raw_collection
        .find(filter.clone())
        .sort(sort)
        .skip(skip)
        .limit(limit)
        .await?;

    let mut games: Vec<HistoryGame> = Vec::new();
    let mut skipped = 0;

    while cursor.advance().await? {
        let raw_doc = cursor.deserialize_current()?;
        match mongodb::bson::from_document::<HistoryGame>(raw_doc.clone()) {
            Ok(game) => games.push(game),
            Err(e) => {
                let match_id = raw_doc
                    .get_str("matchId")
                    .unwrap_or("<unknown>")
                    .to_string();

                let mut stripped = raw_doc;
                stripped.remove("lineups");

                match mongodb::bson::from_document::<HistoryGame>(stripped) {
                    Ok(game) => {
                        tracing::warn!(
                            "⚠️ Recovered history fixture {} by dropping malformed 'lineups' field",
                            match_id
                        );
                        games.push(game);
                    }
                    Err(e2) => {
                        skipped += 1;
                        tracing::warn!(
                            "⚠️ Skipping malformed history document {}: {} (original: {})",
                            match_id,
                            e2,
                            e
                        );
                    }
                }
            }
        }
    }

    if skipped > 0 {
        tracing::warn!("⚠️ Skipped {} malformed history document(s)", skipped);
    }

    let total = collection.count_documents(doc! {}).await? as i64;
    Ok((games, total))
}

// ============================================================================
// COMBINED READ HELPERS
// ============================================================================

async fn find_games_tolerant_all(state: &AppState, filter: Document) -> Result<Vec<Game>> {
    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);

    let mut games = find_games_tolerant(&games_col, filter.clone()).await?;
    let mut fixtures = find_games_tolerant(&fixtures_col, filter).await?;
    games.append(&mut fixtures);
    Ok(games)
}

async fn find_game_tolerant_all(state: &AppState, filter: Document) -> Result<Option<Game>> {
    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    if let Some(g) = find_game_tolerant(&games_col, filter.clone()).await? {
        return Ok(Some(g));
    }
    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);
    find_game_tolerant(&fixtures_col, filter).await
}

// ============================================================================
// COMBINED WRITE HELPERS
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameSource {
    Games,
    Fixtures,
}

impl GameSource {
    fn collection_name(&self) -> &'static str {
        match self {
            GameSource::Games => GAMES_COLLECTION,
            GameSource::Fixtures => FIXTURES_COLLECTION,
        }
    }

    fn collection(&self, state: &AppState) -> Collection<Game> {
        state.db.collection(self.collection_name())
    }
}

async fn update_one_in_either(
    state: &AppState,
    filter: Document,
    update: Document,
) -> Result<GameSource> {
    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    let result = games_col.update_one(filter.clone(), update.clone()).await?;
    if result.matched_count > 0 {
        return Ok(GameSource::Games);
    }

    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);
    let result = fixtures_col.update_one(filter, update).await?;
    if result.matched_count > 0 {
        return Ok(GameSource::Fixtures);
    }

    Err(AppError::DocumentNotFound)
}

async fn update_one_in_either_opt(
    state: &AppState,
    filter: Document,
    update: Document,
) -> Result<Option<GameSource>> {
    match update_one_in_either(state, filter, update).await {
        Ok(source) => Ok(Some(source)),
        Err(AppError::DocumentNotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

async fn delete_one_in_either(state: &AppState, filter: Document) -> Result<GameSource> {
    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    let result = games_col.delete_one(filter.clone()).await?;
    if result.deleted_count > 0 {
        return Ok(GameSource::Games);
    }

    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);
    let result = fixtures_col.delete_one(filter).await?;
    if result.deleted_count > 0 {
        return Ok(GameSource::Fixtures);
    }

    Err(AppError::DocumentNotFound)
}

// ============================================================================
// TEST NOTIFICATION
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TestNotificationRequest {
    pub r#type: String,
    pub message: String,
    pub timestamp: String,
}

pub async fn send_test_notification_from_poller(
    State(state): State<AppState>,
    Json(payload): Json<TestNotificationRequest>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🔔 Received test notification request from poller");
    send_startup_test_notification(State(state)).await
}

pub async fn send_startup_test_notification(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🔔 Sending startup test notification...");

    let now_eat = (Utc::now() + chrono::FixedOffset::east(3 * 3600))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let title = "⚽ FanClash Live Poller is ACTIVE!";
    let body = format!(
        "Your match notifications are now live. Time: {} EAT",
        now_eat
    );
    let notification_type = "test_startup";

    let state_clone = state.clone();
    let now_eat_clone = now_eat.clone();

    tokio::spawn(async move {
        let fcm_tokens_col: Collection<FCMToken> = state_clone.db.collection("fcm_tokens");
        let cursor = match fcm_tokens_col.find(doc! {}).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("❌ Failed to query tokens: {}", e);
                return;
            }
        };

        let tokens = match cursor.try_collect::<Vec<FCMToken>>().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("❌ Failed to collect tokens: {}", e);
                return;
            }
        };

        let mut latest_tokens: HashMap<String, FCMToken> = HashMap::new();
        for token in tokens {
            let entry = latest_tokens.entry(token.user_id.clone());
            match entry {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(token);
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    if token.updated_at > o.get().updated_at {
                        o.insert(token);
                    }
                }
            }
        }

        let fcm_service = match state_clone.fcm_service.as_ref() {
            Some(s) => s,
            None => {
                tracing::error!("❌ FCM service not initialized");
                return;
            }
        };

        for (user_id, _token) in latest_tokens {
            let _ = fcm_service
                .send_to_user(
                    &state_clone,
                    &user_id,
                    title,
                    &body,
                    json!({
                        "timestamp": now_eat_clone,
                        "test": true,
                        "type": notification_type
                    }),
                    notification_type,
                )
                .await;
        }
    });

    Ok(Json(json!({
        "success": true,
        "message": "Test notification started in background",
        "timestamp": now_eat
    })))
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn parse_kickoff_utc(date_iso: &str, time_str: &str) -> Option<chrono::DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(date_iso, "%Y-%m-%d").ok()?;
    let time = NaiveTime::parse_from_str(time_str, "%H:%M").ok()?;
    let naive = NaiveDateTime::new(date, time);
    let utc = chrono::FixedOffset::east_opt(3 * 3600)?
        .from_local_datetime(&naive)
        .single()?
        .with_timezone(&Utc);
    Some(utc)
}

// ============================================================================
// GAME HANDLERS
// ============================================================================

pub async fn get_games(
    State(state): State<AppState>,
    Query(query): Query<GameQuery>,
) -> Result<Json<Vec<Game>>> {
    tracing::info!("🔍 GET /api/games called (leagues + national teams)");

    let mut filter = doc! {};

    if query.status.is_none() && query.is_live.is_none() {
        filter.insert("status", doc! { "$ne": "completed" });
    }

    if let Some(status) = &query.status {
        filter.insert("status", status);
    }
    if let Some(league) = &query.league {
        filter.insert("league", league);
    }
    if let Some(is_live) = query.is_live {
        filter.insert("isLive", is_live);
    }

    let mut games = find_games_tolerant_all(&state, filter).await?;

    games.sort_by(|a, b| a.kickoff_utc.cmp(&b.kickoff_utc));
    let limit = query.limit.unwrap_or(100).max(0) as usize;
    games.truncate(limit);

    tracing::info!("✅ Returning {} games", games.len());
    Ok(Json(games))
}

pub async fn get_game_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Game>> {
    let filter = doc! { "_id": &id };

    match find_game_tolerant_all(&state, filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn get_game_by_match_id(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<Game>> {
    let filter = doc! { "matchId": &match_id };

    match find_game_tolerant_all(&state, filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn get_live_games(State(state): State<AppState>) -> Result<Json<Vec<Game>>> {
    let filter = doc! { "status": "live", "isLive": true };

    let mut live_games = find_games_tolerant_all(&state, filter).await?;
    live_games.sort_by(|a, b| a.kickoff_utc.cmp(&b.kickoff_utc));

    tracing::info!("✅ Fetched {} live games", live_games.len());
    Ok(Json(live_games))
}

pub async fn get_upcoming_games(State(state): State<AppState>) -> Result<Json<Vec<Game>>> {
    let filter = doc! { "status": "upcoming" };

    let games = find_games_tolerant_all(&state, filter).await?;

    let now = Utc::now();
    const MATCH_DURATION_MINS: i64 = 120;

    let mut not_started: Vec<Game> = Vec::new();
    let mut likely_over: Vec<Game> = Vec::new();

    for game in games {
        match parse_kickoff_utc(&game.date_iso, &game.time) {
            Some(kickoff) => {
                let end_estimate = kickoff + chrono::Duration::minutes(MATCH_DURATION_MINS);
                if end_estimate < now {
                    likely_over.push(game);
                } else {
                    not_started.push(game);
                }
            }
            None => not_started.push(game),
        }
    }

    not_started.sort_by(|a, b| {
        let ka = format!("{} {}", a.date_iso, a.time);
        let kb = format!("{} {}", b.date_iso, b.time);
        ka.cmp(&kb)
    });

    likely_over.sort_by(|a, b| {
        let ka = format!("{} {}", a.date_iso, a.time);
        let kb = format!("{} {}", b.date_iso, b.time);
        kb.cmp(&ka)
    });

    let mut sorted: Vec<Game> = not_started;
    sorted.extend(likely_over);

    tracing::info!("✅ Returning {} upcoming games", sorted.len());
    Ok(Json(sorted))
}

// ============================================================================
// UPDATE GAME SCORE - UPDATED (timeElapsed only)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct UpdateGameScoreRequest {
    #[serde(rename = "matchId")]
    pub match_id: String,
    #[serde(rename = "homeScore")]
    pub home_score: Option<i32>,
    #[serde(rename = "awayScore")]
    pub away_score: Option<i32>,
    pub status: Option<String>,
    #[serde(rename = "isLive")]
    pub is_live: Option<bool>,
    #[serde(rename = "timeElapsed")]
    pub time_elapsed: Option<f64>,
}

pub async fn update_game_score(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Json(payload): Json<UpdateGameScoreRequest>,
) -> Result<Json<Game>> {
    use crate::models::channel::ChannelFixture;

    let filter = doc! { "matchId": &match_id };
    let mut update_doc = doc! {};

    if let Some(home_score) = payload.home_score {
        update_doc.insert("homeScore", home_score);
    }
    if let Some(away_score) = payload.away_score {
        update_doc.insert("awayScore", away_score);
    }
    if let Some(status) = &payload.status {
        update_doc.insert("status", status);
    }
    if let Some(is_live) = payload.is_live {
        update_doc.insert("isLive", is_live);
    }
    if let Some(time_elapsed) = payload.time_elapsed {
        update_doc.insert("timeElapsed", time_elapsed);
    }
    update_doc.insert("scrapedAt", BsonDateTime::from_chrono(Utc::now()));

    let source = update_one_in_either(&state, filter.clone(), doc! { "$set": update_doc }).await?;
    let collection = source.collection(&state);

    if payload.home_score.is_some() || payload.away_score.is_some() {
        if let Some(game) = find_game_tolerant(&collection, filter.clone()).await? {
            let channel_fixtures_col: Collection<ChannelFixture> =
                state.db.collection("channel_fixtures");
            let mut cursor = channel_fixtures_col
                .find(doc! { "fixture_id": &match_id })
                .await?;

            let score_payload = json!({
                "fixture_id": match_id,
                "home_score": game.home_score.unwrap_or(0),
                "away_score": game.away_score.unwrap_or(0),
                "time_elapsed": game.time_elapsed,
            });

            let mut channel_count = 0;
            while cursor.advance().await? {
                let cf = cursor.deserialize_current()?;
                broadcast_live_match_update(
                    &state,
                    &cf.channel_id,
                    &match_id,
                    "score",
                    score_payload.clone(),
                )
                .await;
                channel_count += 1;
            }

            tracing::info!(
                "📡 Broadcasted score update for {} ({:?}) to {} channels: {}-{}",
                match_id,
                source,
                channel_count,
                game.home_score.unwrap_or(0),
                game.away_score.unwrap_or(0)
            );
        }
    }

    match find_game_tolerant(&collection, filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

// ============================================================================
// UPDATE GAME STATUS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GameStatusUpdate {
    pub status: String,
}

pub async fn update_game_status(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Json(payload): Json<GameStatusUpdate>,
) -> Result<Json<Game>> {
    use crate::handlers::channel::finalize_fixture_result_handler;
    use crate::handlers::channel::FinalizeFixtureRequest;
    use crate::handlers::ws_handler::broadcast_live_match_update;
    use crate::models::channel::ChannelFixture;

    let valid_statuses = ["upcoming", "soon", "live", "completed"];
    if !valid_statuses.contains(&payload.status.as_str()) {
        return Err(AppError::invalid_data(&format!(
            "Invalid status. Must be one of: {:?}",
            valid_statuses
        )));
    }

    let is_live = payload.status == "live";
    let available_for_voting = matches!(payload.status.as_str(), "upcoming" | "soon");

    let filter = doc! { "matchId": &match_id };
    let update = doc! { "$set": {
        "status": &payload.status,
        "isLive": is_live,
        "availableForVoting": available_for_voting,
        "scrapedAt": BsonDateTime::from_chrono(Utc::now()),
    }};

    let source = update_one_in_either(&state, filter.clone(), update).await?;
    let collection = source.collection(&state);

    if payload.status == "completed" {
        tracing::info!("🏁 Match {} ({:?}) auto-finalizing...", match_id, source);

        if let Some(game) = find_game_tolerant(&collection, filter.clone()).await? {
            let home_score = game.home_score.unwrap_or(0);
            let away_score = game.away_score.unwrap_or(0);

            let result = if home_score > away_score {
                "home"
            } else if away_score > home_score {
                "away"
            } else {
                "draw"
            };

            let finalize_request = FinalizeFixtureRequest {
                fixture_id: match_id.clone(),
                result: result.to_string(),
            };

            let _ =
                finalize_fixture_result_handler(State(state.clone()), Json(finalize_request)).await;
        }
    }

    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let mut cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &match_id })
        .await?;

    let status_payload = json!({
        "fixture_id": match_id,
        "status": payload.status,
        "is_live": is_live,
        "available_for_voting": available_for_voting,
    });

    let mut channel_count = 0;
    while cursor.advance().await? {
        let cf = cursor.deserialize_current()?;
        broadcast_live_match_update(
            &state,
            &cf.channel_id,
            &match_id,
            "status",
            status_payload.clone(),
        )
        .await;
        channel_count += 1;
    }

    match find_game_tolerant(&collection, filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

// ============================================================================
// RECEIVE LIVE UPDATE - UPDATED (timeElapsed only)
// ============================================================================

pub async fn receive_live_update(
    State(state): State<AppState>,
    Json(update): Json<LiveGameUpdate>,
) -> Result<Json<serde_json::Value>> {
    use crate::handlers::channel::finalize_fixture_result_handler;
    use crate::handlers::channel::FinalizeFixtureRequest;
    use crate::handlers::ws_handler::broadcast_live_match_update;
    use crate::models::channel::ChannelFixture;

    tracing::info!("🔴 Live update received: {:?}", update);

    let filter = doc! { "matchId": &update.fixture_id };

    let (status, is_live, available_for_voting) = match update.event_type.as_str() {
        "match_end" => ("completed", false, false),
        "half_time" => ("live", true, false),
        "second_half" => ("live", true, false),
        _ => ("live", true, false),
    };

    let mut set_doc = doc! {
        "homeScore": update.home_score,
        "awayScore": update.away_score,
        "status": status,
        "isLive": is_live,
        "availableForVoting": available_for_voting,
        "scrapedAt": BsonDateTime::from_chrono(Utc::now()),
    };

    // ✅ Use time_elapsed if provided, otherwise use minute as fallback
    if let Some(time_elapsed) = update.time_elapsed {
        set_doc.insert("timeElapsed", time_elapsed);
    } else {
        set_doc.insert("timeElapsed", update.minute as f64);
    }

    let source = update_one_in_either(&state, filter.clone(), doc! { "$set": set_doc }).await?;

    tracing::info!(
        "✅ Updated {} ({:?}): {}-{} (status: {}, time: {:.1}')",
        update.fixture_id,
        source,
        update.home_score,
        update.away_score,
        status,
        update.time_elapsed.unwrap_or(update.minute as f64)
    );

    if update.event_type == "match_end" {
        let result = if update.home_score > update.away_score {
            "home"
        } else if update.away_score > update.home_score {
            "away"
        } else {
            "draw"
        };

        let finalize_request = FinalizeFixtureRequest {
            fixture_id: update.fixture_id.clone(),
            result: result.to_string(),
        };

        let _ = finalize_fixture_result_handler(State(state.clone()), Json(finalize_request)).await;
    }

    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let mut cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &update.fixture_id })
        .await?;

    let update_payload = json!({
        "fixture_id": update.fixture_id,
        "event_type": update.event_type,
        "home_score": update.home_score,
        "away_score": update.away_score,
        "time_elapsed": update.time_elapsed.unwrap_or(update.minute as f64),
        "scorer": update.scorer,
        "player": update.player,
        "assist": update.assist,
        "team": update.team,
        "status": status,
        "is_live": is_live,
    });

    let mut channel_count = 0;
    while cursor.advance().await? {
        let cf = cursor.deserialize_current()?;
        broadcast_live_match_update(
            &state,
            &cf.channel_id,
            &update.fixture_id,
            &update.event_type,
            update_payload.clone(),
        )
        .await;
        channel_count += 1;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Live update processed and broadcasted",
        "fixture_id": update.fixture_id,
        "event_type": update.event_type,
        "status": status,
        "source_collection": source.collection_name(),
        "channels_notified": channel_count,
    })))
}

// ============================================================================
// LINEUPS HANDLERS
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct LineupsUpdate {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,
    #[serde(rename = "homeTeam")]
    pub home_team: String,
    #[serde(rename = "awayTeam")]
    pub away_team: String,
    pub lineups: LineupsPayload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineupsPayload {
    pub home: TeamLineupPayload,
    pub away: TeamLineupPayload,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamLineupPayload {
    pub formation: String,
    pub coach: CoachPayload,
    pub players: Vec<PlayerPayload>,
    pub bench: Vec<PlayerPayload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoachPayload {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerPayload {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: String,
    #[serde(rename = "playerId")]
    pub player_id: Option<String>,
}

fn map_players(players: Vec<PlayerPayload>, default_lineup: &str) -> Vec<Player> {
    players
        .into_iter()
        .map(|p| Player {
            name: p.name,
            position: p.position,
            jersey_number: p.jersey_number,
            captain: p.captain,
            lineup: if p.lineup.is_empty() {
                default_lineup.to_string()
            } else {
                p.lineup
            },
            player_id: p.player_id,
            rating: None,
        })
        .collect()
}

pub async fn store_lineups(
    State(state): State<AppState>,
    Json(payload): Json<LineupsUpdate>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📋 Storing lineups for fixture: {}", payload.fixture_id);

    let home_lineup = TeamLineup {
        formation: payload.lineups.home.formation.clone(),
        coach: Coach {
            name: payload.lineups.home.coach.name.clone(),
        },
        players: map_players(payload.lineups.home.players.clone(), "starting"),
        bench: map_players(payload.lineups.home.bench.clone(), "bench"),
    };

    let away_lineup = TeamLineup {
        formation: payload.lineups.away.formation.clone(),
        coach: Coach {
            name: payload.lineups.away.coach.name.clone(),
        },
        players: map_players(payload.lineups.away.players.clone(), "starting"),
        bench: map_players(payload.lineups.away.bench.clone(), "bench"),
    };

    let home_player_count = home_lineup.players.len();
    let away_player_count = away_lineup.players.len();

    let doc = LineupsDocument::new(
        payload.fixture_id.clone(),
        payload.home_team.clone(),
        payload.away_team.clone(),
        home_lineup,
        away_lineup,
    );

    let bson_doc = bson::to_bson(&doc).map_err(|e| {
        AppError::InternalServerError(format!("Failed to serialize lineups: {}", e))
    })?;

    let source = update_one_in_either(
        &state,
        doc! { "matchId": &payload.fixture_id },
        doc! { "$set": {
            "lineups": bson_doc,
            "lineupsFetched": true,
            "lineupsFetchedAt": BsonDateTime::from_chrono(Utc::now()),
        }},
    )
    .await?;

    tracing::info!(
        "✅ Lineups embedded in {:?} fixture document for {} (home: {}, away: {})",
        source,
        payload.fixture_id,
        home_player_count,
        away_player_count
    );

    Ok(Json(json!({
        "success": true,
        "fixture_id": payload.fixture_id,
        "source_collection": source.collection_name(),
        "home_players": home_player_count,
        "away_players": away_player_count,
    })))
}

pub async fn get_lineups(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    match game.lineups {
        Some(doc) => {
            let data = lineup_doc_to_json(&doc);
            Ok(Json(json!({ "success": true, "data": data })))
        }
        None => Ok(Json(json!({ "success": false, "data": null }))),
    }
}

pub async fn get_simplified_lineups(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    match game.lineups {
        Some(doc) => Ok(Json(json!({
            "success": true,
            "data": lineup_doc_to_json(&doc),
        }))),
        None => Ok(Json(json!({ "success": false, "data": null }))),
    }
}

pub async fn check_lineups_available(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "available": game.lineups.is_some(),
        "fixture_id": match_id,
    })))
}

fn lineup_doc_to_json(doc: &LineupsDocument) -> serde_json::Value {
    json!({
        "home_formation": doc.home_lineup.formation,
        "away_formation": doc.away_lineup.formation,
        "home_coach": doc.home_lineup.coach.name,
        "away_coach": doc.away_lineup.coach.name,
        "home_starting_xi": doc.home_lineup.players.iter().map(|p| json!({
            "name": p.name,
            "number": p.jersey_number,
            "position": p.position,
            "captain": p.captain,
        })).collect::<Vec<_>>(),
        "home_bench": doc.home_lineup.bench.iter().map(|p| json!({
            "name": p.name,
            "number": p.jersey_number,
            "position": p.position,
            "captain": p.captain,
        })).collect::<Vec<_>>(),
        "away_starting_xi": doc.away_lineup.players.iter().map(|p| json!({
            "name": p.name,
            "number": p.jersey_number,
            "position": p.position,
            "captain": p.captain,
        })).collect::<Vec<_>>(),
        "away_bench": doc.away_lineup.bench.iter().map(|p| json!({
            "name": p.name,
            "number": p.jersey_number,
            "position": p.position,
            "captain": p.captain,
        })).collect::<Vec<_>>(),
        "fetched_at": doc.fetched_at.to_chrono().to_rfc3339(),
    })
}

// ============================================================================
// STATISTICS HANDLERS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct StatisticsSnapshotPayload {
    pub fixture_id: String,
    pub minute: i32,
    pub statistics: MatchStatisticsPayload,
}

#[derive(Debug, Deserialize)]
pub struct MatchStatisticsPayload {
    pub home: TeamStatisticsPayload,
    pub away: TeamStatisticsPayload,
}

#[derive(Debug, Deserialize)]
pub struct TeamStatisticsPayload {
    pub possession: Option<f64>,
    pub shots: Option<i32>,
    pub shots_on_target: Option<i32>,
    pub shots_off_target: Option<i32>,
    pub corners: Option<i32>,
    pub fouls: Option<i32>,
    pub yellow_cards: Option<i32>,
    pub red_cards: Option<i32>,
    pub offsides: Option<i32>,
    pub passes: Option<i32>,
    pub pass_accuracy: Option<f64>,
}

fn team_stats_from_payload(p: TeamStatisticsPayload) -> TeamStatistics {
    TeamStatistics {
        possession: p.possession,
        shots: p.shots,
        shots_on_target: p.shots_on_target,
        shots_off_target: p.shots_off_target,
        corners: p.corners,
        fouls: p.fouls,
        yellow_cards: p.yellow_cards,
        red_cards: p.red_cards,
        offsides: p.offsides,
        passes: p.passes,
        pass_accuracy: p.pass_accuracy,
    }
}

fn team_stats_from_json(s: &serde_json::Value) -> TeamStatistics {
    TeamStatistics {
        possession: s["possession"].as_f64(),
        shots: s["shots"].as_i64().map(|v| v as i32),
        shots_on_target: s["shots_on_target"].as_i64().map(|v| v as i32),
        shots_off_target: s["shots_off_target"].as_i64().map(|v| v as i32),
        corners: s["corners"].as_i64().map(|v| v as i32),
        fouls: s["fouls"].as_i64().map(|v| v as i32),
        yellow_cards: s["yellow_cards"].as_i64().map(|v| v as i32),
        red_cards: s["red_cards"].as_i64().map(|v| v as i32),
        offsides: s["offsides"].as_i64().map(|v| v as i32),
        passes: s["passes"].as_i64().map(|v| v as i32),
        pass_accuracy: s["pass_accuracy"].as_f64(),
    }
}

pub async fn add_statistics_snapshot(
    State(state): State<AppState>,
    Json(payload): Json<StatisticsSnapshotPayload>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📊 Adding statistics snapshot for {}", payload.fixture_id);

    let snapshot = StatisticsSnapshot {
        minute: payload.minute,
        statistics: MatchStatistics {
            home: team_stats_from_payload(payload.statistics.home),
            away: team_stats_from_payload(payload.statistics.away),
        },
        timestamp: BsonDateTime::from_chrono(Utc::now()),
    };

    let bson_snapshot = bson::to_bson(&snapshot).map_err(|e| {
        AppError::InternalServerError(format!("Failed to serialize statistics: {}", e))
    })?;

    let source = update_one_in_either(
        &state,
        doc! { "matchId": &payload.fixture_id },
        doc! {
            "$push": { "statistics": bson_snapshot },
            "$set": {
                "lastStatisticsMinute": payload.minute,
                "scrapedAt": BsonDateTime::from_chrono(Utc::now()),
            }
        },
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": payload.fixture_id,
        "source_collection": source.collection_name(),
        "minute": payload.minute,
    })))
}

pub async fn bulk_update_statistics(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📊 Bulk statistics update");

    let fixture_id = payload["fixture_id"]
        .as_str()
        .ok_or_else(|| AppError::invalid_data("Missing fixture_id"))?;

    let snapshots = payload["snapshots"]
        .as_array()
        .ok_or_else(|| AppError::invalid_data("Missing snapshots array"))?;

    let mut bson_snapshots = Vec::new();
    for snapshot in snapshots {
        let minute = snapshot["minute"].as_i64().unwrap_or(0) as i32;
        let stats = &snapshot["statistics"];

        let s = StatisticsSnapshot {
            minute,
            statistics: MatchStatistics {
                home: team_stats_from_json(&stats["home"]),
                away: team_stats_from_json(&stats["away"]),
            },
            timestamp: BsonDateTime::from_chrono(Utc::now()),
        };

        let bson_s = bson::to_bson(&s).map_err(|e| {
            AppError::InternalServerError(format!("Failed to serialize statistics: {}", e))
        })?;
        bson_snapshots.push(bson_s);
    }

    let last_minute = snapshots
        .last()
        .and_then(|s| s["minute"].as_i64())
        .unwrap_or(0) as i32;

    let source = update_one_in_either(
        &state,
        doc! { "matchId": fixture_id },
        doc! {
            "$push": { "statistics": { "$each": bson_snapshots } },
            "$set": {
                "lastStatisticsMinute": last_minute,
                "scrapedAt": BsonDateTime::from_chrono(Utc::now()),
            }
        },
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "source_collection": source.collection_name(),
        "count": snapshots.len(),
    })))
}

pub async fn get_match_statistics(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": match_id,
        "statistics": game.statistics,
        "count": game.statistics.len(),
    })))
}

pub async fn get_latest_statistics(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    if let Some(latest) = game.statistics.last() {
        Ok(Json(json!({
            "ball_possession_home": latest.statistics.home.possession.unwrap_or(0.0),
            "ball_possession_away": latest.statistics.away.possession.unwrap_or(0.0),
            "total_shots_home": latest.statistics.home.shots.unwrap_or(0),
            "total_shots_away": latest.statistics.away.shots.unwrap_or(0),
            "shots_on_target_home": latest.statistics.home.shots_on_target.unwrap_or(0),
            "shots_on_target_away": latest.statistics.away.shots_on_target.unwrap_or(0),
            "shots_off_target_home": latest.statistics.home.shots_off_target.unwrap_or(0),
            "shots_off_target_away": latest.statistics.away.shots_off_target.unwrap_or(0),
            "corners_home": latest.statistics.home.corners.unwrap_or(0),
            "corners_away": latest.statistics.away.corners.unwrap_or(0),
            "fouls_home": latest.statistics.home.fouls.unwrap_or(0),
            "fouls_away": latest.statistics.away.fouls.unwrap_or(0),
            "yellow_cards_home": latest.statistics.home.yellow_cards.unwrap_or(0),
            "yellow_cards_away": latest.statistics.away.yellow_cards.unwrap_or(0),
            "red_cards_home": latest.statistics.home.red_cards.unwrap_or(0),
            "red_cards_away": latest.statistics.away.red_cards.unwrap_or(0),
            "offsides_home": latest.statistics.home.offsides.unwrap_or(0),
            "offsides_away": latest.statistics.away.offsides.unwrap_or(0),
            "passes_home": latest.statistics.home.passes.unwrap_or(0),
            "passes_away": latest.statistics.away.passes.unwrap_or(0),
            "pass_accuracy_home": latest.statistics.home.pass_accuracy.unwrap_or(0.0),
            "pass_accuracy_away": latest.statistics.away.pass_accuracy.unwrap_or(0.0),
            "minute": latest.minute,
        })))
    } else {
        Ok(Json(json!({
            "ball_possession_home": 0.0,
            "ball_possession_away": 0.0,
            "total_shots_home": 0,
            "total_shots_away": 0,
            "shots_on_target_home": 0,
            "shots_on_target_away": 0,
            "shots_off_target_home": 0,
            "shots_off_target_away": 0,
            "corners_home": 0,
            "corners_away": 0,
            "fouls_home": 0,
            "fouls_away": 0,
            "yellow_cards_home": 0,
            "yellow_cards_away": 0,
            "red_cards_home": 0,
            "red_cards_away": 0,
            "offsides_home": 0,
            "offsides_away": 0,
            "passes_home": 0,
            "passes_away": 0,
            "pass_accuracy_home": 0.0,
            "pass_accuracy_away": 0.0,
            "minute": 0,
        })))
    }
}

pub async fn get_statistics_at_minute(
    State(state): State<AppState>,
    Path((match_id, minute)): Path<(String, i32)>,
) -> Result<Json<serde_json::Value>> {
    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let stats_at_minute = game.statistics.iter().find(|s| s.minute == minute);

    Ok(Json(json!({
        "success": true,
        "fixture_id": match_id,
        "minute": minute,
        "statistics": stats_at_minute,
    })))
}

// ============================================================================
// COMMENTARY HANDLERS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CommentaryUpdate {
    pub match_id: String,
    pub entry: CommentaryEntry,
}

#[derive(Debug, Deserialize)]
pub struct LatestCommentaryQuery {
    pub limit: Option<i64>,
}

pub async fn add_commentary(
    State(state): State<AppState>,
    Json(payload): Json<CommentaryUpdate>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📝 Adding commentary for match: {}", payload.match_id);

    let now = BsonDateTime::from_chrono(Utc::now());
    let mut entry = payload.entry;
    entry.created_at = now;

    let bson_entry = bson::to_bson(&entry).map_err(|e| {
        AppError::InternalServerError(format!("Failed to serialize commentary: {}", e))
    })?;

    let source = update_one_in_either(
        &state,
        doc! { "matchId": &payload.match_id },
        doc! {
            "$push": { "commentary": bson_entry },
            "$inc": { "commentary_count": 1 },
            "$set": { "lastCommentaryAt": now }
        },
    )
    .await?;

    let broadcast_msg = json!({
        "type": "commentary.new",
        "payload": entry,
        "timestamp": Utc::now().to_rfc3339(),
    });

    use crate::models::channel::ChannelFixture;
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let mut cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.match_id })
        .await?;

    let broadcast_json = serde_json::to_string(&broadcast_msg).unwrap();
    let mut channel_count = 0;

    while cursor.advance().await? {
        let cf = cursor.deserialize_current()?;
        let room_key = format!("{}_{}", cf.channel_id, payload.match_id);
        let tx = state.get_or_create_broadcaster(&room_key);
        let _ = tx.send(broadcast_json.clone());
        channel_count += 1;
    }

    tracing::info!(
        "✅ Commentary stored in {:?} and broadcasted to {} channels",
        source,
        channel_count
    );

    Ok(Json(json!({
        "success": true,
        "message": "Commentary added",
        "source_collection": source.collection_name(),
        "channels_notified": channel_count,
    })))
}

pub async fn get_latest_commentary(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Query(params): Query<LatestCommentaryQuery>,
) -> Result<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(20);

    let game = find_game_tolerant_all(&state, doc! { "matchId": &match_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let mut commentary = game.commentary.clone();
    commentary.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let latest: Vec<CommentaryEntry> = commentary.into_iter().take(limit as usize).collect();

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "commentary": latest,
        "total_count": game.commentary.len() as i64,
    })))
}

#[derive(Debug, Deserialize)]
pub struct CommentaryBulkUpdate {
    pub match_id: String,
    pub entries: Vec<CommentaryEntry>,
}

pub async fn add_commentary_bulk(
    State(state): State<AppState>,
    Json(payload): Json<CommentaryBulkUpdate>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        "📝 Adding {} bulk commentary entries for match: {}",
        payload.entries.len(),
        payload.match_id
    );

    if payload.entries.is_empty() {
        return Ok(Json(json!({
            "success": true,
            "message": "No entries to add",
            "count": 0,
        })));
    }

    let mut bson_entries = Vec::with_capacity(payload.entries.len());
    let mut broadcast_entries = Vec::with_capacity(payload.entries.len());

    for mut entry in payload.entries {
        entry.created_at = BsonDateTime::from_chrono(Utc::now());
        let bson_entry = bson::to_bson(&entry).map_err(|e| {
            AppError::InternalServerError(format!("Failed to serialize commentary entry: {}", e))
        })?;
        broadcast_entries.push(entry);
        bson_entries.push(bson_entry);
    }

    let count = bson_entries.len() as i64;
    let now = BsonDateTime::from_chrono(Utc::now());

    let source = update_one_in_either(
        &state,
        doc! { "matchId": &payload.match_id },
        doc! {
            "$push": { "commentary": { "$each": bson_entries } },
            "$inc": { "commentary_count": count },
            "$set": { "lastCommentaryAt": now }
        },
    )
    .await?;

    let broadcast_msg = json!({
        "type": "commentary.bulk",
        "payload": broadcast_entries,
        "timestamp": Utc::now().to_rfc3339(),
    });

    use crate::models::channel::ChannelFixture;
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let mut cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.match_id })
        .await?;

    let broadcast_json = serde_json::to_string(&broadcast_msg).unwrap();
    let mut channel_count = 0;

    while cursor.advance().await? {
        let cf = cursor.deserialize_current()?;
        let room_key = format!("{}_{}", cf.channel_id, payload.match_id);
        let tx = state.get_or_create_broadcaster(&room_key);
        let _ = tx.send(broadcast_json.clone());
        channel_count += 1;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Bulk commentary added",
        "source_collection": source.collection_name(),
        "count": count,
        "channels_notified": channel_count,
    })))
}

// ============================================================================
// HISTORY / ARCHIVE HANDLERS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub skip: Option<u64>,
    pub league: Option<String>,
    pub home_team: Option<String>,
    pub away_team: Option<String>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
}

pub async fn move_completed_to_history(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📦 Moving completed game {} to history", match_id);

    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);

    let game_opt = find_game_tolerant(
        &games_col,
        doc! { "matchId": &match_id, "status": "completed" },
    )
    .await?;
    let source_collection = if game_opt.is_some() {
        GameSource::Games
    } else {
        let fixture_opt = find_game_tolerant(
            &fixtures_col,
            doc! { "matchId": &match_id, "status": "completed" },
        )
        .await?;
        if fixture_opt.is_some() {
            GameSource::Fixtures
        } else {
            let games_history_col: Collection<HistoryGame> = state.db.collection("games_history");
            let fixtures_history_col: Collection<HistoryGame> =
                state.db.collection("fixtures_history");

            if games_history_col
                .find_one(doc! { "matchId": &match_id })
                .await?
                .is_some()
                || fixtures_history_col
                    .find_one(doc! { "matchId": &match_id })
                    .await?
                    .is_some()
            {
                tracing::info!("✅ Game {} already in history (idempotent no-op)", match_id);
                return Ok(Json(json!({
                    "success": true,
                    "message": format!("Game {} already moved to history", match_id),
                    "match_id": match_id,
                })));
            }
            return Err(AppError::DocumentNotFound);
        }
    };

    let game = match game_opt {
        Some(g) => g,
        None => {
            let fixture_opt = find_game_tolerant(
                &fixtures_col,
                doc! { "matchId": &match_id, "status": "completed" },
            )
            .await?;
            match fixture_opt {
                Some(f) => f,
                None => return Err(AppError::DocumentNotFound),
            }
        }
    };

    let match_id_clone = game.match_id.clone();

    let history_game = HistoryGame {
        id: game.id.clone(),
        match_id: game.match_id.clone(),
        threesixtyfive_game_id: game.threesixtyfive_game_id.clone(),
        home_team: game.home_team.clone(),
        away_team: game.away_team.clone(),
        league: game.league.clone(),
        home_win: game.home_win,
        away_win: game.away_win,
        draw: game.draw,
        date: game.date.clone(),
        time: game.time.clone(),
        date_iso: game.date_iso.clone(),
        kickoff_utc: game.kickoff_utc,
        home_score: game.home_score,
        away_score: game.away_score,
        status: game.status.clone(),
        is_live: game.is_live,
        available_for_voting: game.available_for_voting,
        time_elapsed: game.time_elapsed,
        result: game.result.clone(),
        source: game.source.clone(),
        scraped_at: game.scraped_at,
        last_scraped_at: game.last_scraped_at,
        last_polled_at: game.last_polled_at,
        commentary: game.commentary.clone(),
        last_commentary_at: game.last_commentary_at,
        lineups: game.lineups,
        lineups_fetched: game.lineups_fetched,
        lineups_fetched_at: game.lineups_fetched_at,
        statistics: game.statistics,
        last_statistics_minute: game.last_statistics_minute,
        forwarded_event_signatures: game.forwarded_event_signatures,
        completed_at: BsonDateTime::from_chrono(Utc::now()),
        moved_to_history: true,
        created_at: game.created_at,
    };

    let history_col_name = match source_collection {
        GameSource::Games => "games_history",
        GameSource::Fixtures => "fixtures_history",
    };

    let history_col: Collection<HistoryGame> = state.db.collection(history_col_name);
    history_col
        .replace_one(doc! { "matchId": &match_id_clone }, history_game)
        .upsert(true)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to upsert history for {}: {}", match_id_clone, e);
            e
        })?;

    let source_col = source_collection.collection(&state);
    source_col
        .delete_one(doc! { "matchId": &match_id_clone })
        .await?;

    tracing::info!("✅ Game {} moved to {}", match_id, history_col_name);

    Ok(Json(json!({
        "success": true,
        "message": format!("Game {} moved to history", match_id),
        "match_id": match_id,
        "source_collection": source_collection.collection_name(),
        "history_collection": history_col_name,
    })))
}

pub async fn get_history_games(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📜 GET /api/games/history called (leagues + national teams)");

    let games_history_col: Collection<HistoryGame> = state.db.collection("games_history");
    let fixtures_history_col: Collection<HistoryGame> = state.db.collection("fixtures_history");

    let mut filter = doc! {};

    if let Some(league) = &query.league {
        filter.insert("league", league);
    }
    if let Some(home_team) = &query.home_team {
        filter.insert("homeTeam", home_team);
    }
    if let Some(away_team) = &query.away_team {
        filter.insert("awayTeam", away_team);
    }

    let mut completed_at_filter = Document::new();
    if let Some(from_date) = &query.from_date {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(from_date, "%Y-%m-%d") {
            let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                date.and_hms_opt(0, 0, 0).unwrap(),
                Utc,
            );
            completed_at_filter.insert("$gte", BsonDateTime::from_chrono(dt));
        }
    }
    if let Some(to_date) = &query.to_date {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(to_date, "%Y-%m-%d") {
            let dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                date.and_hms_opt(23, 59, 59).unwrap(),
                Utc,
            );
            completed_at_filter.insert("$lte", BsonDateTime::from_chrono(dt));
        }
    }
    if !completed_at_filter.is_empty() {
        filter.insert("completedAt", completed_at_filter);
    }

    let limit = query.limit.unwrap_or(50);
    let skip = query.skip.unwrap_or(0);
    let sort = doc! { "completedAt": -1 };

    let (games_history, games_total) = find_history_games_tolerant(
        &games_history_col,
        filter.clone(),
        sort.clone(),
        skip,
        limit,
    )
    .await?;

    let (fixtures_history, fixtures_total) = find_history_games_tolerant(
        &fixtures_history_col,
        filter.clone(),
        sort.clone(),
        skip,
        limit,
    )
    .await?;

    let mut all_history = Vec::new();
    all_history.extend(games_history);
    all_history.extend(fixtures_history);

    all_history.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));

    let total = games_total + fixtures_total;
    let skip_usize = skip as usize;
    let limit_usize = limit as usize;

    let paginated: Vec<HistoryGame> = all_history
        .into_iter()
        .skip(skip_usize)
        .take(limit_usize)
        .collect();

    tracing::info!(
        "✅ Retrieved {} history games ({} from games_history, {} from fixtures_history)",
        paginated.len(),
        games_total,
        fixtures_total
    );

    Ok(Json(json!({
        "success": true,
        "data": paginated,
        "total": total,
        "limit": limit,
        "skip": skip,
        "sources": {
            "games_history": games_total,
            "fixtures_history": fixtures_total
        },
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_history_game_by_match_id(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let games_history_col: Collection<HistoryGame> = state.db.collection("games_history");
    let fixtures_history_col: Collection<HistoryGame> = state.db.collection("fixtures_history");

    let filter = doc! { "matchId": &match_id };

    let (mut games, _) =
        find_history_games_tolerant(&games_history_col, filter.clone(), doc! {}, 0, 1).await?;

    if let Some(game) = games.pop() {
        return Ok(Json(json!({
            "success": true,
            "data": game,
            "source": "games_history"
        })));
    }

    let (mut fixtures, _) =
        find_history_games_tolerant(&fixtures_history_col, filter, doc! {}, 0, 1).await?;

    match fixtures.pop() {
        Some(game) => Ok(Json(json!({
            "success": true,
            "data": game,
            "source": "fixtures_history"
        }))),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn cleanup_stale_completed_games(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🧹 Cleaning up stale completed games (leagues + national teams)");

    let history_col: Collection<HistoryGame> = state.db.collection("games_history");

    let one_hour_ago = BsonDateTime::from_chrono(Utc::now() - chrono::Duration::hours(1));
    let stale_filter = doc! {
        "status": "completed",
        "scrapedAt": { "$lt": one_hour_ago }
    };

    let games_col: Collection<Game> = state.db.collection(GAMES_COLLECTION);
    let fixtures_col: Collection<Game> = state.db.collection(FIXTURES_COLLECTION);

    let stale_games = find_games_tolerant(&games_col, stale_filter.clone()).await?;
    let stale_fixtures = find_games_tolerant(&fixtures_col, stale_filter).await?;

    let mut moved_count = 0;

    for (games, source) in [
        (stale_games, GameSource::Games),
        (stale_fixtures, GameSource::Fixtures),
    ] {
        for game in games {
            let match_id = game.match_id.clone();

            let history_game = HistoryGame {
                id: game.id.clone(),
                match_id: game.match_id.clone(),
                threesixtyfive_game_id: game.threesixtyfive_game_id.clone(),
                home_team: game.home_team.clone(),
                away_team: game.away_team.clone(),
                league: game.league.clone(),
                home_win: game.home_win,
                away_win: game.away_win,
                draw: game.draw,
                date: game.date.clone(),
                time: game.time.clone(),
                date_iso: game.date_iso.clone(),
                kickoff_utc: game.kickoff_utc,
                home_score: game.home_score,
                away_score: game.away_score,
                status: game.status.clone(),
                is_live: game.is_live,
                available_for_voting: game.available_for_voting,
                time_elapsed: game.time_elapsed,
                result: game.result.clone(),
                source: game.source.clone(),
                scraped_at: game.scraped_at,
                last_scraped_at: game.last_scraped_at,
                last_polled_at: game.last_polled_at,
                commentary: game.commentary.clone(),
                last_commentary_at: game.last_commentary_at,
                lineups: game.lineups,
                lineups_fetched: game.lineups_fetched,
                lineups_fetched_at: game.lineups_fetched_at,
                statistics: game.statistics,
                last_statistics_minute: game.last_statistics_minute,
                forwarded_event_signatures: game.forwarded_event_signatures,
                completed_at: BsonDateTime::from_chrono(Utc::now()),
                moved_to_history: true,
                created_at: game.created_at,
            };

            history_col.insert_one(history_game).await?;

            let collection = source.collection(&state);
            collection.delete_one(doc! { "matchId": &match_id }).await?;
            moved_count += 1;
        }
    }

    tracing::info!("✅ Moved {} stale completed games to history", moved_count);

    Ok(Json(json!({
        "success": true,
        "message": format!("Moved {} stale games to history", moved_count),
        "moved_count": moved_count,
    })))
}
