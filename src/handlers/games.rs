use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tracing; // Add if missing

use crate::errors::{AppError, Result};
use crate::handlers::ws_handler::broadcast_live_match_update;
use crate::models::game::{
    CommentaryEntry, Game, GameQuery, GameStatusUpdate, HistoryGame, LiveGameUpdate,
    UpdateGameScore, Voter,
};
use crate::models::notification::FCMToken;
use crate::state::AppState;

// ============================================================================
// TEST NOTIFICATION REQUEST
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
    tracing::info!("=======================================================");
    tracing::info!("🔔 Received test notification request from poller");
    tracing::info!("  Type: {}, Message: {}", payload.r#type, payload.message);
    tracing::info!("=======================================================");

    send_startup_test_notification(State(state)).await
}

pub async fn send_startup_test_notification(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("=======================================================");
    tracing::info!("🔔 Received test notification request - processing in background");
    tracing::info!("=======================================================");

    let now_eat = (Utc::now() + chrono::FixedOffset::east(3 * 3600))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    tracing::info!("📅 Current time (EAT): {}", now_eat);

    let title = "⚽ FanClash Live Poller is ACTIVE!";
    let body = format!(
        "Your match notifications are now live. Time: {} EAT",
        now_eat
    );
    let notification_type = "test_startup";

    let now_eat_clone = now_eat.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        tracing::info!("📱 BACKGROUND: Sending test notifications...");

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

        tracing::info!(
            "📊 Found {} unique users (latest token only)",
            latest_tokens.len()
        );

        let fcm_service = match state_clone.fcm_service.as_ref() {
            Some(s) => s,
            None => {
                tracing::error!("❌ FCM service not initialized");
                return;
            }
        };

        let mut sent_count = 0;
        for (user_id, token) in latest_tokens {
            tracing::info!(
                "📱 Sending to user: {} (token: {}...)",
                user_id,
                &token.fcm_token[0..10.min(token.fcm_token.len())]
            );

            match fcm_service
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
                .await
            {
                Ok(success) => {
                    if success {
                        sent_count += 1;
                        tracing::info!("✅ Sent to user: {}", user_id);
                    } else {
                        tracing::warn!("⚠️ No valid token for user: {}", user_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("❌ Failed to send to user {}: {}", user_id, e);
                }
            }
        }

        tracing::info!(
            "✅ BACKGROUND: Test notifications sent to {} users",
            sent_count
        );
    });

    Ok(Json(json!({
        "success": true,
        "message": "Test notification started in background (1 per user)",
        "timestamp": now_eat
    })))
}

// ============================================================================
// LINEUP AVAILABLE NOTIFICATION
// ============================================================================

pub async fn send_lineup_available_notification(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        "📋 Received lineup notification request for match: {}",
        match_id
    );

    let match_id_clone = match_id.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        tracing::info!("📱 BACKGROUND: Sending lineup notifications...");

        let games_col: Collection<Game> = state_clone.db.collection("fixtures");
        let filter = doc! { "match_id": &match_id_clone };

        let game = match games_col.find_one(filter).await {
            Ok(Some(g)) => g,
            Ok(None) => {
                tracing::warn!("⚠️ Game not found: {}", match_id_clone);
                return;
            }
            Err(e) => {
                tracing::error!("❌ Failed to fetch game: {}", e);
                return;
            }
        };

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

        let unique_users: std::collections::HashSet<String> =
            tokens.iter().map(|t| t.user_id.clone()).collect();

        let title = format!(
            "📋 Lineups are out! {} vs {}",
            game.home_team, game.away_team
        );
        let body = format!(
            "Check the starting XI for {} vs {}. Who will win?",
            game.home_team, game.away_team
        );
        let notification_type = "lineups_available";

        let fcm_service = match state_clone.fcm_service.as_ref() {
            Some(s) => s,
            None => {
                tracing::error!("❌ FCM service not initialized");
                return;
            }
        };

        for user_id in unique_users {
            let _ = fcm_service
                .send_to_user(
                    &state_clone,
                    &user_id,
                    &title,
                    &body,
                    json!({
                        "fixture_id": match_id_clone,
                        "home_team": game.home_team,
                        "away_team": game.away_team,
                        "type": notification_type
                    }),
                    notification_type,
                )
                .await;
        }
        tracing::info!("✅ BACKGROUND: Lineup notifications complete");
    });

    Ok(Json(json!({
        "success": true,
        "message": "Lineup notification started in background",
        "fixture_id": match_id
    })))
}

// ============================================================================
// HYPE NOTIFICATIONS
// ============================================================================

pub async fn check_and_send_hype_notifications(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📅 Checking for hype notifications");

    let state_clone = state.clone();

    tokio::spawn(async move {
        tracing::info!("📱 BACKGROUND: Checking hype notifications...");

        let games_col: Collection<Game> = state_clone.db.collection("fixtures");
        let now = Utc::now();

        let cursor = match games_col.find(doc! { "status": "upcoming" }).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("❌ Failed to query games: {}", e);
                return;
            }
        };

        let games: Vec<Game> = match cursor.try_collect().await {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("❌ Failed to collect games: {}", e);
                return;
            }
        };

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

        let unique_users: std::collections::HashSet<String> =
            tokens.iter().map(|t| t.user_id.clone()).collect();
        let fcm_service = match state_clone.fcm_service.as_ref() {
            Some(s) => s,
            None => {
                tracing::error!("❌ FCM service not initialized");
                return;
            }
        };

        for game in games {
            let kickoff = parse_kickoff_utc(&game.date_iso, &game.time);
            if let Some(kickoff) = kickoff {
                let days_until = (kickoff - now).num_days();

                let (title, body, days) = if days_until == 14 {
                    (
                        format!("🎉 2 weeks until {} vs {}!", game.home_team, game.away_team),
                        format!(
                            "Kickoff at {}",
                            (kickoff + chrono::FixedOffset::east(3 * 3600))
                                .format("%A, %B %d at %H:%M")
                        ),
                        14,
                    )
                } else if days_until == 7 {
                    (
                        format!("📅 1 week to go! {} vs {}", game.home_team, game.away_team),
                        format!(
                            "Kickoff at {}",
                            (kickoff + chrono::FixedOffset::east(3 * 3600))
                                .format("%A, %B %d at %H:%M")
                        ),
                        7,
                    )
                } else if days_until == 1 {
                    (
                        format!(
                            "⏰ 24 hours until {} vs {}!",
                            game.home_team, game.away_team
                        ),
                        format!(
                            "Kickoff tomorrow at {}",
                            (kickoff + chrono::FixedOffset::east(3 * 3600)).format("%H:%M")
                        ),
                        1,
                    )
                } else {
                    continue;
                };

                for user_id in &unique_users {
                    let _ = fcm_service
                        .send_to_user(
                            &state_clone,
                            user_id,
                            &title,
                            &body,
                            json!({
                                "fixture_id": game.match_id,
                                "days_until": days,
                                "type": "hype"
                            }),
                            "hype",
                        )
                        .await;
                }
            }
        }
        tracing::info!("✅ BACKGROUND: Hype notifications complete");
    });

    Ok(Json(json!({
        "success": true,
        "message": "Hype notifications check started in background"
    })))
}

// ============================================================================
// COUNTDOWN NOTIFICATIONS
// ============================================================================

pub async fn check_and_send_countdown_notifications(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("⏰ Checking for countdown notifications");

    let state_clone = state.clone();

    tokio::spawn(async move {
        tracing::info!("📱 BACKGROUND: Checking countdown notifications...");

        let games_col: Collection<Game> = state_clone.db.collection("fixtures");
        let now = Utc::now();

        let cursor = match games_col.find(doc! { "status": "upcoming" }).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("❌ Failed to query games: {}", e);
                return;
            }
        };

        let games: Vec<Game> = match cursor.try_collect().await {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("❌ Failed to collect games: {}", e);
                return;
            }
        };

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

        let unique_users: std::collections::HashSet<String> =
            tokens.iter().map(|t| t.user_id.clone()).collect();
        let fcm_service = match state_clone.fcm_service.as_ref() {
            Some(s) => s,
            None => {
                tracing::error!("❌ FCM service not initialized");
                return;
            }
        };

        for game in games {
            let kickoff = parse_kickoff_utc(&game.date_iso, &game.time);
            if let Some(kickoff) = kickoff {
                let minutes_until = (kickoff - now).num_minutes();
                let kickoff_eat = (kickoff + chrono::FixedOffset::east(3 * 3600)).format("%H:%M");
                let name = format!("{} vs {}", game.home_team, game.away_team);

                let (title, body) = if minutes_until == 60 {
                    (
                        "🔔 1 hour until kick-off!".to_string(),
                        format!(
                            "{} kicks off at {} EAT. Pick your side! ⚽",
                            name, kickoff_eat
                        ),
                    )
                } else if minutes_until == 45 {
                    (
                        "⏰ 45 minutes to kick-off!".to_string(),
                        format!("{} at {} EAT — get your votes in! 🎯", name, kickoff_eat),
                    )
                } else if minutes_until == 30 {
                    (
                        "⚡ 30 minutes to go!".to_string(),
                        format!("{} — rivalries heating up. Who's winning this? 🔥", name),
                    )
                } else if minutes_until == 10 {
                    (
                        "🔥 10 minutes! Last chance to vote!".to_string(),
                        format!("{} — locks soon. Don't miss out! ⏰", name),
                    )
                } else {
                    continue;
                };

                for user_id in &unique_users {
                    let _ = fcm_service
                        .send_to_user(
                            &state_clone,
                            user_id,
                            &title,
                            &body,
                            json!({
                                "fixture_id": game.match_id,
                                "minutes_to_kickoff": minutes_until,
                                "type": "countdown"
                            }),
                            "countdown",
                        )
                        .await;
                }
            }
        }
        tracing::info!("✅ BACKGROUND: Countdown notifications complete");
    });

    Ok(Json(json!({
        "success": true,
        "message": "Countdown notifications check started in background"
    })))
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

async fn send_goal_notification_to_voter(
    state: &AppState,
    voter: &Voter,
    fixture: &Game,
    update: &LiveGameUpdate,
) -> Result<()> {
    let fcm_service = state
        .fcm_service
        .as_ref()
        .ok_or_else(|| AppError::InternalServerError("FCM service not initialized".to_string()))?;

    let home_team = &fixture.home_team;
    let away_team = &fixture.away_team;
    let scored_team = if update.scorer == Some("home_team".to_string()) {
        home_team
    } else {
        away_team
    };
    let score_line = format!("{}-{}", update.home_score, update.away_score);

    let (title, body, ntype) = if Some(&voter.selection) == update.scorer.as_ref() {
        (
            format!("⚽ GOAL! Your team scored!"),
            format!("{} scores! {} ({})", scored_team, score_line, update.minute),
            "goal_your_team".to_string(),
        )
    } else if voter.selection == "draw" {
        (
            format!("⚽ Goal! Draw under pressure"),
            format!(
                "{} scores → {} ({})",
                scored_team, score_line, update.minute
            ),
            "goal_draw_pressure".to_string(),
        )
    } else {
        (
            format!("⚔️ RIVAL SCORED!"),
            format!(
                "Your rival's team ({}) scored! {} ({})",
                scored_team, score_line, update.minute
            ),
            "goal_rival_team".to_string(),
        )
    };

    let success = fcm_service
        .send_to_user(
            state,
            &voter.user_id,
            &title,
            &body,
            json!({
                "fixture_id": fixture.match_id,
                "home_score": update.home_score,
                "away_score": update.away_score,
                "minute": update.minute,
                "type": ntype
            }),
            &ntype,
        )
        .await?;

    if success {
        tracing::info!("✅ Goal notification sent to user: {}", voter.user_id);
    } else {
        tracing::warn!("⚠️ No tokens for user: {}", voter.user_id);
    }

    Ok(())
}

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
    tracing::info!("🔍 GET /api/games called with query: {:?}", query);

    let collection: Collection<Game> = state.db.collection("fixtures");
    let mut filter = doc! {};

    if let Some(status) = &query.status {
        filter.insert("status", status);
    }
    if let Some(league) = &query.league {
        filter.insert("league", league);
    }
    if let Some(is_live) = query.is_live {
        filter.insert("is_live", is_live);
    }

    let cursor = collection.find(filter).await?;
    let mut games: Vec<Game> = cursor.try_collect().await?;

    games.sort_by(|a, b| b.scraped_at.cmp(&a.scraped_at));

    tracing::info!("✅ Fetched {} games", games.len());
    Ok(Json(games))
}

pub async fn get_game_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Game>> {
    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "_id": &id };

    match collection.find_one(filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn get_game_by_match_id(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<Game>> {
    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &match_id };

    match collection.find_one(filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn get_live_games(State(state): State<AppState>) -> Result<Json<Vec<Game>>> {
    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "status": "live", "is_live": true };

    let cursor = collection.find(filter).await?;
    let live_games: Vec<Game> = cursor.try_collect().await?;

    tracing::info!("✅ Fetched {} live games", live_games.len());
    Ok(Json(live_games))
}

pub async fn get_upcoming_games(State(state): State<AppState>) -> Result<Json<Vec<Game>>> {
    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "status": "upcoming" };

    let cursor = collection.find(filter).await?;
    let games: Vec<Game> = cursor.try_collect().await?;

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

pub async fn update_game_score(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Json(payload): Json<UpdateGameScore>,
) -> Result<Json<Game>> {
    use crate::handlers::ws_handler::broadcast_live_match_update;
    use crate::models::channel::ChannelFixture;
    use mongodb::Collection;

    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &match_id };
    let mut update_doc = doc! {};

    if let Some(home_score) = payload.home_score {
        update_doc.insert("home_score", home_score);
    }
    if let Some(away_score) = payload.away_score {
        update_doc.insert("away_score", away_score);
    }
    if let Some(status) = &payload.status {
        update_doc.insert("status", status);
    }
    if let Some(is_live) = payload.is_live {
        update_doc.insert("is_live", is_live);
    }
    if let Some(time_elapsed) = payload.time_elapsed {
        update_doc.insert("time_elapsed", time_elapsed);
    }
    update_doc.insert("scraped_at", BsonDateTime::from_chrono(Utc::now()));

    let update_result = collection
        .update_one(filter.clone(), doc! { "$set": update_doc })
        .await?;
    
    if update_result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    // ✅ Broadcast to ALL channels that have this fixture
    if payload.home_score.is_some() || payload.away_score.is_some() {
        if let Some(game) = collection.find_one(filter.clone()).await? {
            let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
            let mut cursor = channel_fixtures_col
                .find(doc! { "fixture_id": &match_id })
                .await?;

            let score_payload = json!({
                "fixture_id": match_id,
                "home_score": game.home_score.unwrap_or(0),
                "away_score": game.away_score.unwrap_or(0),
                "minute": game.time_elapsed,
            });

            let mut channel_count = 0;
            while cursor.advance().await? {
                let cf = cursor.deserialize_current()?;
                broadcast_live_match_update(
                    &state, 
                    &cf.channel_id, 
                    &match_id, 
                    "score", 
                    score_payload.clone()
                ).await;
                channel_count += 1;
            }

            tracing::info!(
                "📡 Broadcasted score update for {} to {} channels: {}-{}",
                match_id,
                channel_count,
                game.home_score.unwrap_or(0),
                game.away_score.unwrap_or(0)
            );
        }
    }

    match collection.find_one(filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}
// ============================================================================
// UPDATE GAME STATUS - Broadcasts to ALL channels with this fixture
// ============================================================================

pub async fn update_game_status(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Json(payload): Json<GameStatusUpdate>,
) -> Result<Json<Game>> {
    use crate::handlers::ws_handler::broadcast_live_match_update;
    use crate::models::channel::ChannelFixture;
    use mongodb::Collection;

    let collection: Collection<Game> = state.db.collection("fixtures");

    let valid_statuses = ["upcoming", "soon", "live", "completed"];
    if !valid_statuses.contains(&payload.status.as_str()) {
        return Err(AppError::invalid_data(&format!(
            "Invalid status. Must be one of: {:?}",
            valid_statuses
        )));
    }

    let is_live = payload.status == "live";
    let available_for_voting = matches!(payload.status.as_str(), "upcoming" | "soon");

    let filter = doc! { "match_id": &match_id };
    let update = doc! { "$set": {
        "status": &payload.status,
        "is_live": is_live,
        "available_for_voting": available_for_voting,
        "scraped_at": BsonDateTime::from_chrono(Utc::now()),
    }};

    collection.update_one(filter.clone(), update).await?;

    // ✅ Broadcast to ALL channels that have this fixture
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
        ).await;
        channel_count += 1;
    }

    tracing::info!(
        "📡 Broadcasted status update for {} to {} channels: {}",
        match_id,
        channel_count,
        payload.status
    );

    match collection.find_one(filter).await? {
        Some(game) => Ok(Json(game)),
        None => Err(AppError::DocumentNotFound),
    }
}

// ============================================================================
// RECEIVE LIVE UPDATE - From Python Poller, Broadcasts to ALL channels
// ============================================================================

pub async fn receive_live_update(
    State(state): State<AppState>,
    Json(update): Json<LiveGameUpdate>,
) -> Result<Json<serde_json::Value>> {
    use crate::handlers::ws_handler::broadcast_live_match_update;
    use crate::models::channel::ChannelFixture;
    use mongodb::Collection;

    tracing::info!("🔴 Live update received: {:?}", update);

    let games_col: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &update.fixture_id };

    // Determine status based on event_type
    let (status, is_live, available_for_voting) = match update.event_type.as_str() {
        "match_end" => ("completed", false, false),
        "half_time" => ("live", true, false),
        "second_half" => ("live", true, false),
        _ => ("live", true, false),
    };

    let set_doc = doc! {
        "home_score": update.home_score,
        "away_score": update.away_score,
        "time_elapsed": update.minute,
        "status": status,
        "is_live": is_live,
        "available_for_voting": available_for_voting,
        "scraped_at": BsonDateTime::from_chrono(Utc::now()),
    };

    games_col
        .update_one(filter.clone(), doc! { "$set": set_doc })
        .await?;

    tracing::info!(
        "✅ Updated {}: {}-{} (status: {}, {}')",
        update.fixture_id,
        update.home_score,
        update.away_score,
        status,
        update.minute
    );

    // ✅ Broadcast to ALL channels that have this fixture
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let mut cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &update.fixture_id })
        .await?;

    let update_payload = json!({
        "fixture_id": update.fixture_id,
        "event_type": update.event_type,
        "home_score": update.home_score,
        "away_score": update.away_score,
        "minute": update.minute,
        "minute_display": update.minute_display,
        "scorer": update.scorer,
        "player": update.player,
        "assist": update.assist,
        "team": update.team,
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
        ).await;
        channel_count += 1;
    }

    tracing::info!(
        "📡 Broadcasted {} event for {} to {} channels",
        update.event_type,
        update.fixture_id,
        channel_count
    );

    Ok(Json(json!({
        "success": true,
        "message": "Live update processed and broadcasted",
        "fixture_id": update.fixture_id,
        "event_type": update.event_type,
        "status": status,
        "channels_notified": channel_count,
    })))
}
// ============================================================================
// FAST COUNT HANDLERS
// ============================================================================

pub async fn get_fixture_vote_count_fast(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &fixture_id };

    let game = games_collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "votes": game.votes,
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_fixture_comment_count_fast(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &fixture_id };

    let game = games_collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "comments": game.comments,
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_fixture_counts_fast(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &fixture_id };

    let game = games_collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "votes": game.votes,
        "comments": game.comments,
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_batch_fixture_counts_fast(
    State(state): State<AppState>,
    Json(fixture_ids): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let mut results = Vec::new();
    let mut error_count = 0;

    for fixture_id in fixture_ids {
        let filter = doc! { "match_id": &fixture_id };

        match games_collection.find_one(filter).await {
            Ok(Some(game)) => {
                results.push(json!({
                    "fixture_id": fixture_id,
                    "votes": game.votes,
                    "comments": game.comments,
                }));
            }
            Ok(None) => {
                results.push(json!({
                    "fixture_id": fixture_id,
                    "votes": 0,
                    "comments": 0,
                }));
            }
            Err(e) => {
                error_count += 1;
                results.push(json!({
                    "fixture_id": fixture_id,
                    "votes": 0,
                    "comments": 0,
                    "error": format!("{}", e)
                }));
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "count": results.len(),
        "data": results,
        "errors": error_count,
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_fixture_voters_fast(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &fixture_id };

    let game = games_collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    let voters: Vec<serde_json::Value> = game
        .voters
        .iter()
        .map(|v| {
            json!({
                "userId": v.user_id,
                "userName": v.user_name,
                "selection": v.selection,
                "votedAt": v.voted_at.to_chrono().to_rfc3339(),
            })
        })
        .collect();

    let home_votes = voters
        .iter()
        .filter(|v| v["selection"] == "home_team")
        .count();
    let draw_votes = voters.iter().filter(|v| v["selection"] == "draw").count();
    let away_votes = voters
        .iter()
        .filter(|v| v["selection"] == "away_team")
        .count();

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "home_team": game.home_team,
        "away_team": game.away_team,
        "total_votes": game.votes,
        "voters": voters,
        "breakdown": {
            "home": home_votes,
            "draw": draw_votes,
            "away": away_votes,
        },
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn check_user_voted_fast(
    State(state): State<AppState>,
    Path((fixture_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let games_collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! {
        "match_id": &fixture_id,
        "voters.userId": &user_id
    };

    let game = games_collection.find_one(filter).await?;

    let has_voted = game.is_some();
    let selection = if let Some(game) = game {
        game.voters
            .iter()
            .find(|v| v.user_id == user_id)
            .map(|v| v.selection.clone())
    } else {
        None
    };

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "user_id": user_id,
        "has_voted": has_voted,
        "selection": selection,
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

    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &payload.match_id };

    let now = BsonDateTime::from_chrono(Utc::now());
    let mut entry = payload.entry;
    entry.created_at = now;

    let update = doc! {
        "$push": { "commentary": bson::to_bson(&entry).unwrap() },
        "$inc": { "commentary_count": 1 },
        "$set": { "last_commentary_at": now }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    // Broadcast to WebSocket
    let broadcast_msg = json!({
        "type": "commentary.new",
        "payload": entry,
        "timestamp": Utc::now().to_rfc3339(),
    });

    let tx = state.get_or_create_broadcaster(&payload.match_id);
    let _ = tx.send(serde_json::to_string(&broadcast_msg).unwrap());

    tracing::info!("✅ Commentary stored and broadcasted");

    Ok(Json(json!({
        "success": true,
        "message": "Commentary added"
    })))
}


pub async fn get_match_commentary(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &match_id };

    let game = collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    let mut commentary = game.commentary;
    commentary.sort_by(|a, b| a.minute.cmp(&b.minute));

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "commentary": commentary,
        "commentary_count": game.commentary_count,
        "last_commentary_at": game.last_commentary_at,
    })))
}

pub async fn get_latest_commentary(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
    Query(params): Query<LatestCommentaryQuery>,
) -> Result<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(20);

    let collection: Collection<Game> = state.db.collection("fixtures");
    let filter = doc! { "match_id": &match_id };

    let game = collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    let mut commentary = game.commentary;
    commentary.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let latest: Vec<CommentaryEntry> = commentary.into_iter().take(limit as usize).collect();

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "commentary": latest,
        "total_count": game.commentary_count,
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

    let games_col: Collection<Game> = state.db.collection("fixtures");
    let history_col: Collection<HistoryGame> = state.db.collection("games_history");

    let filter = doc! { "match_id": &match_id, "status": "completed" };
    let game = games_col.find_one(filter.clone()).await?;

    if let Some(game) = game {
        let match_id_clone = game.match_id.clone(); // ✅ Clone before moving

        let history_entry = HistoryGame {
            game, // game moved here
            completed_at: BsonDateTime::from_chrono(Utc::now()),
            moved_to_history: true,
        };

        history_col.insert_one(history_entry).await?;
        games_col
            .delete_one(doc! { "match_id": &match_id_clone })
            .await?; // ✅ Use cloned value

        tracing::info!("✅ Game {} moved to history", match_id);

        Ok(Json(json!({
            "success": true,
            "message": format!("Game {} moved to history", match_id),
            "match_id": match_id,
        })))
    } else {
        Err(AppError::DocumentNotFound)
    }
}

pub async fn get_history_games(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📜 GET /api/games/history called");

    let collection: Collection<HistoryGame> = state.db.collection("games_history");
    let mut filter = doc! {};

    if let Some(league) = &query.league {
        filter.insert("league", league);
    }
    if let Some(home_team) = &query.home_team {
        filter.insert("home_team", home_team);
    }
    if let Some(away_team) = &query.away_team {
        filter.insert("away_team", away_team);
    }

    // ✅ Fixed date filtering
    if let Some(from_date) = &query.from_date {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(from_date, "%Y-%m-%d") {
            let datetime = date.and_hms_opt(0, 0, 0).unwrap();
            let utc_datetime = chrono::DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc);
            let bson_date = BsonDateTime::from_chrono(utc_datetime);
            filter.insert("completed_at", doc! { "$gte": bson_date });
        }
    }
    if let Some(to_date) = &query.to_date {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(to_date, "%Y-%m-%d") {
            let datetime = date.and_hms_opt(23, 59, 59).unwrap();
            let utc_datetime = chrono::DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc);
            let bson_date = BsonDateTime::from_chrono(utc_datetime);
            filter.insert("completed_at", doc! { "$lte": bson_date });
        }
    }

    let limit = query.limit.unwrap_or(50);
    let skip = query.skip.unwrap_or(0);

    let cursor = collection
        .find(filter)
        .sort(doc! { "completed_at": -1 })
        .skip(skip)
        .limit(limit)
        .await?;

    let history_games: Vec<HistoryGame> = cursor.try_collect().await?;
    let total = collection.count_documents(doc! {}).await?;

    tracing::info!("✅ Retrieved {} history games", history_games.len());

    Ok(Json(json!({
        "success": true,
        "data": history_games,
        "total": total,
        "limit": limit,
        "skip": skip,
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

pub async fn get_history_game_by_match_id(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<HistoryGame> = state.db.collection("games_history");
    let filter = doc! { "match_id": &match_id };

    match collection.find_one(filter).await? {
        Some(game) => Ok(Json(json!({
            "success": true,
            "data": game,
        }))),
        None => Err(AppError::DocumentNotFound),
    }
}

pub async fn cleanup_stale_completed_games(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🧹 Cleaning up stale completed games");

    let games_col: Collection<Game> = state.db.collection("fixtures");
    let history_col: Collection<HistoryGame> = state.db.collection("games_history");

    let one_hour_ago = BsonDateTime::from_chrono(Utc::now() - chrono::Duration::hours(1));
    let filter = doc! {
        "status": "completed",
        "scraped_at": doc! { "$lt": one_hour_ago }
    };

    let stale_games: Vec<Game> = games_col.find(filter.clone()).await?.try_collect().await?;
    let mut moved_count = 0;

    for game in stale_games {
        let match_id = game.match_id.clone(); // ✅ Clone first before moving

        let history_entry = HistoryGame {
            game, // game moved here
            completed_at: BsonDateTime::from_chrono(Utc::now()),
            moved_to_history: true,
        };

        history_col.insert_one(history_entry).await?;
        games_col.delete_one(doc! { "match_id": &match_id }).await?; // ✅ Use cloned value
        moved_count += 1;
    }

    tracing::info!("✅ Moved {} stale completed games to history", moved_count);

    Ok(Json(json!({
        "success": true,
        "message": format!("Moved {} stale games to history", moved_count),
        "moved_count": moved_count,
    })))
}
// ============================================================================
// LIVE UPDATE HANDLER (Called by Python Poller)
// ============================================================================
