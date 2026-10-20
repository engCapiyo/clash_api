use crate::{
    errors::AppError,
    handlers::sub_fixture_handler::resolve_market_id,
    models::{
        actions::{
            Bet, CastVoteRequest, CreateBetRequest, CreatePokeRequest, FillBetRequest,
            FillPokeRequest, Poke, PokeListQuery, RollbackVoteRequest, SettleBetRequest,
            SettlePokeRequest, Vote, MIN_POKE_POINTS,
        },
        channel::{Channel, ChannelFixture, VoteCounts},
        game::Game,
        sub_fixture::SubFixtureMarket,
        user::User,
    },
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use bson::{doc, to_bson, Bson, DateTime as BsonDateTime, Document};
use futures_util::StreamExt;
use mongodb::{ClientSession, Collection};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

// ============================================================================
// HELPER: Get all channel IDs where user is a member
// ============================================================================
async fn get_user_channel_ids(
    channels_col: &Collection<Channel>,
    user_id: &str,
) -> Result<Vec<String>, AppError> {
    tracing::debug!("🔍 Finding channels for user: {}", user_id);

    let mut cursor = channels_col
        .find(doc! { "members.user_id": user_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channels: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut channel_ids = Vec::new();
    while let Some(channel) = cursor.next().await {
        let channel: Channel = channel.map_err(|e| {
            tracing::error!("❌ Failed to deserialize channel: {}", e);
            AppError::MongoDB(e)
        })?;

        let channel_id = channel.channel_id.clone();
        tracing::debug!("📌 Found channel: {}", channel_id);
        channel_ids.push(channel_id);
    }

    tracing::info!("✅ User {} is in {} channels", user_id, channel_ids.len());
    Ok(channel_ids)
}

// ============================================================================
// HELPER: Get all channel members
// ============================================================================
async fn get_channel_members(
    channels_col: &Collection<Channel>,
    channel_id: &str,
) -> Result<Vec<String>, AppError> {
    let channel = channels_col
        .find_one(doc! { "channel_id": channel_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

    Ok(channel.members.iter().map(|m| m.user_id.clone()).collect())
}

// ============================================================================
// HELPER: Send WebSocket broadcast
// ============================================================================
async fn broadcast_to_room(
    state: &AppState,
    room_key: &str,
    event_type: &str,
    payload: serde_json::Value,
) {
    let tx = state.get_or_create_broadcaster(room_key);

    let ws_message = serde_json::json!({
        "type": event_type,
        "payload": payload,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    if let Ok(json) = serde_json::to_string(&ws_message) {
        let _ = tx.send(json);
        tracing::info!("📡 Broadcasted {} to room {}", event_type, room_key);
    }
}

// ============================================================================
// HELPER: Send notification to channel members
// ============================================================================
async fn notify_channel_members(
    state: &AppState,
    actor_id: &str,
    channel_id: &str,
    fixture_id: &str,
    notification_type: &str,
    title: &str,
    body: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let member_ids = get_channel_members(&channels_col, channel_id).await?;

    tracing::info!(
        "🔔 notify_channel_members: channel={}, fixture={}, type={}, members={}",
        channel_id, fixture_id, notification_type, member_ids.len()
    );

    if member_ids.is_empty() {
        return Ok(());
    }

    let room_key = format!("{}_{}", channel_id, fixture_id);
    broadcast_to_room(state, &room_key, notification_type, payload.clone()).await;

    let fcm_service = match &state.fcm_service {
        Some(s) => s,
        None => {
            tracing::warn!("⚠️ FCM service not available — skipping push for channel {}", channel_id);
            return Ok(());
        }
    };

    let data = serde_json::json!({
        "type": notification_type,
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "actor_id": actor_id,
    });

    for user_id in member_ids {
        if user_id == actor_id {
            tracing::debug!("⏭️ Skipping actor {} (self)", user_id);
            continue;
        }

        tracing::info!("📤 Dispatching FCM push to member {}", user_id);
        let _ = fcm_service
            .send_to_user(state, &user_id, title, body, data.clone(), notification_type)
            .await;
    }

    Ok(())
}

// ============================================================================
// HELPER: Create or update ChannelFixture for a channel
// ============================================================================
async fn upsert_channel_fixture(
    channel_fixtures_col: &Collection<ChannelFixture>,
    channel_id: &str,
    fixture_id: &str,
    increment_field: Option<&str>,
    increment_value: i32,
    set_on_insert_status: &str,
) -> Result<(), AppError> {
    tracing::debug!(
        "📝 Upserting channel_fixture: channel={}, fixture={}, field={:?}, value={}",
        channel_id,
        fixture_id,
        increment_field,
        increment_value
    );

    let filter = doc! {
        "channel_id": channel_id,
        "fixture_id": fixture_id,
    };

    let mut update = doc! {};

    let mut set_on_insert = doc! {
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "status": set_on_insert_status,
        "comment_count": 0,
        "likes_count": 0,
        "unread_counts": doc! {},
        "added_at": BsonDateTime::now(),
    };

    let defaultable_fields: [(&str, i32); 6] = [
        ("vote_counts.home", 0),
        ("vote_counts.away", 0),
        ("vote_counts.draw", 0),
        ("pledge_count", 0),
        ("bet_count", 0),
        ("poke_count", 0),
    ];

    for (field, default) in defaultable_fields {
        if increment_field != Some(field) {
            set_on_insert.insert(field, default);
        }
    }

    update.insert("$setOnInsert", set_on_insert);

    if let Some(field) = increment_field {
        let mut inc = doc! {};
        inc.insert(field, increment_value);
        update.insert("$inc", inc);
    }

    let result = channel_fixtures_col
        .update_one(filter, update)
        .upsert(true)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to upsert channel_fixture: {}", e);
            AppError::MongoDB(e)
        })?;

    tracing::debug!(
        "✅ Upsert result: matched={}, modified={}, upserted={:?}",
        result.matched_count,
        result.modified_count,
        result.upserted_id
    );

    Ok(())
}

// ============================================================================
// POKE HELPERS (points-based, no draw)
// ============================================================================

/// Read the spendable points balance from a raw user document.
/// `season_points` is the canonical balance; voting +/-3 and poke stakes all
/// spend/earn this same field.
fn read_points(user: &Document) -> i64 {
    match user.get("season_points") {
        Some(Bson::Int32(v)) => *v as i64,
        Some(Bson::Int64(v)) => *v,
        Some(Bson::Double(v)) => *v as i64,
        _ => 0,
    }
}

/// Map a main-match selection to its vote_counts field.
/// No draw: only home/away are valid selections.
fn vote_count_field(selection: &str) -> Option<&'static str> {
    match selection {
        "home" => Some("vote_counts.home"),
        "away" => Some("vote_counts.away"),
        _ => None,
    }
}

/// Match filter for a main-match poke (market_id null) or a sub-fixture market.
fn poke_target_filter(fixture_id: &str, market_id: &Option<String>) -> Document {
    match market_id {
        Some(m) => doc! { "fixture_id": fixture_id, "market_id": m },
        None => doc! { "fixture_id": fixture_id, "market_id": Bson::Null },
    }
}

fn poke_json(p: &Poke) -> serde_json::Value {
    json!({
        "id": p.id.map(|oid| oid.to_hex()),
        "fixture_id": p.fixture_id,
        "market_id": p.market_id,
        "starter_id": p.starter_id,
        "starter_name": p.starter_name,
        "starter_selection": p.starter_selection,
        "stake": p.stake,
        "finisher_id": p.finisher_id,
        "finisher_name": p.finisher_name,
        "finisher_selection": p.finisher_selection,
        "status": p.status,
        "winner_id": p.winner_id,
        "starter_result": p.starter_result,
        "finisher_result": p.finisher_result,
        "total_pot": p.total_pot(),
        "created_at": p.created_at,
        "matched_at": p.matched_at,
        "settled_at": p.settled_at,
    })
}

struct PokeTarget {
    /// Resolved market business key (e.g. `{match_id}_{market_type}`).
    /// `None` => the poke is on the main match.
    market_id: Option<String>,
    /// Allowed selections. Main match => ["home","away"]. Sub-fixture => market options.
    options: Vec<String>,
}

/// Resolve the poke target and (optionally) enforce that the market is open
/// and the parent match has not started — the same rules as sub-fixture bets.
async fn load_poke_target(
    state: &AppState,
    fixture_id: &str,
    market_id: &Option<String>,
    require_open: bool,
) -> Result<PokeTarget, AppError> {
    let Some(raw) = market_id else {
        return Ok(PokeTarget {
            market_id: None,
            options: vec!["home".to_string(), "away".to_string()],
        });
    };

    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");
    let resolved = resolve_market_id(&markets_col, fixture_id, raw).await?;

    let market = markets_col
        .find_one(doc! { "matchId": fixture_id, "marketId": &resolved })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    if require_open {
        if market.status != "open" {
            return Err(AppError::ValidationError(format!(
                "Market is not open (status: {})",
                market.status
            )));
        }
        let games_col: Collection<Game> = state.db.collection("games");
        let game = games_col
            .find_one(doc! { "matchId": fixture_id })
            .await
            .map_err(AppError::MongoDB)?
            .ok_or(AppError::DocumentNotFound)?;
        if game.status != "upcoming" && game.status != "not_started" {
            return Err(AppError::ValidationError(
                "Match is no longer accepting pokes".to_string(),
            ));
        }
    }

    Ok(PokeTarget {
        market_id: Some(resolved),
        options: market.options,
    })
}

/// Reject draw and any selection not in `options`.
fn check_selection(selection: &str, options: &[String]) -> Result<(), AppError> {
    if selection != "draw" && options.iter().any(|o| o == selection) {
        Ok(())
    } else {
        Err(AppError::ValidationError(format!(
            "Invalid selection. Must be one of: {}",
            options.join(", ")
        )))
    }
}

/// Mirror a points delta onto every channel membership row for this user.
async fn mirror_points_to_channels(
    channels_col: &Collection<Channel>,
    user_id: &str,
    delta: i32,
    session: &mut ClientSession,
) -> Result<(), AppError> {
    channels_col
        .update_many(
            doc! { "members.user_id": user_id },
            doc! { "$inc": { "members.$[m].season_points": delta } },
        )
        .array_filters(vec![doc! { "m.user_id": user_id }])
        .session(&mut *session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to mirror points to channels: {}", e);
            AppError::MongoDB(e)
        })?;
    Ok(())
}

/// Atomically reserve `stake` points. The `season_points >= stake` filter
/// makes two concurrent requests unable to spend the same points twice.
async fn reserve_points(
    users_doc_col: &Collection<Document>,
    channels_col: &Collection<Channel>,
    user_oid: bson::oid::ObjectId,
    user_id: &str,
    stake: i32,
    session: &mut ClientSession,
) -> Result<(), AppError> {
    let res = users_doc_col
        .update_one(
            doc! { "_id": user_oid, "season_points": { "$gte": stake } },
            doc! { "$inc": { "season_points": -stake } },
        )
        .session(&mut *session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to reserve points: {}", e);
            AppError::MongoDB(e)
        })?;

    if res.matched_count == 0 {
        return Err(AppError::ValidationError(
            "Insufficient points".to_string(),
        ));
    }

    mirror_points_to_channels(channels_col, user_id, -stake, session).await
}

/// Give points back / pay out.
async fn credit_points(
    users_doc_col: &Collection<Document>,
    channels_col: &Collection<Channel>,
    user_id: &str,
    amount: i32,
    session: &mut ClientSession,
) -> Result<(), AppError> {
    let oid = bson::oid::ObjectId::parse_str(user_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    users_doc_col
        .update_one(doc! { "_id": oid }, doc! { "$inc": { "season_points": amount } })
        .session(&mut *session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to credit points: {}", e);
            AppError::MongoDB(e)
        })?;

    mirror_points_to_channels(channels_col, user_id, amount, session).await
}

/// Vote/poke consistency: Ok(true) => auto-vote needed, Ok(false) => existing
/// vote matches, Err => mismatch (reject).
fn check_vote_matches(existing: &Option<Vote>, selection: &str) -> Result<bool, AppError> {
    match existing {
        Some(v) if v.selection != selection => Err(AppError::ValidationError(format!(
            "You voted for {}, you cannot poke {}",
            v.selection, selection
        ))),
        Some(_) => Ok(false),
        None => Ok(true),
    }
}

// ============================================================================
// 1. CAST VOTE
// ============================================================================
pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    tracing::info!(
        "🗳️ Cast vote: fixture={}, user={}, selection={}",
        payload.fixture_id,
        payload.user_id,
        payload.selection
    );

    // No draw: only home/away accepted.
    let increment_field = match payload.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        _ => {
            tracing::error!("❌ Invalid selection: {}", payload.selection);
            return Err(AppError::ValidationError(
                "Invalid selection. Must be 'home' or 'away'".to_string(),
            ));
        }
    };

    let existing = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to check existing vote: {}", e);
            AppError::MongoDB(e)
        })?;

    if existing.is_some() {
        tracing::warn!("⚠️ User already voted on this fixture");
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    let vote = Vote::new(
        payload.fixture_id.clone(),
        payload.user_id.clone(),
        payload.username.clone(),
        payload.selection.clone(),
    );

    votes_col.insert_one(&vote).await.map_err(|e| {
        tracing::error!("❌ Failed to insert vote: {}", e);
        AppError::MongoDB(e)
    })?;

    let channel_ids = get_user_channel_ids(&channels_col, &payload.user_id).await?;

    tracing::info!(
        "📊 User is in {} channels, updating vote counts for fixture {}",
        channel_ids.len(),
        payload.fixture_id
    );

    for channel_id in &channel_ids {
        upsert_channel_fixture(
            &channel_fixtures_col,
            channel_id,
            &payload.fixture_id,
            Some(increment_field),
            1,
            "active",
        )
        .await?;
    }

    let vote_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "user_id": payload.user_id,
        "username": payload.username,
        "selection": payload.selection,
    });

    for channel_id in &channel_ids {
        let _ = notify_channel_members(
            &state,
            &payload.user_id,
            channel_id,
            &payload.fixture_id,
            "vote.cast",
            "🗳️ New Vote",
            &format!("{} voted on the match", payload.username),
            vote_payload.clone(),
        )
        .await;
    }

    tracing::info!(
        "✅ Vote cast successfully for user {} in {} channels",
        payload.user_id,
        channel_ids.len()
    );

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully",
        "fixture_id": payload.fixture_id,
        "selection": payload.selection,
        "vote_id": payload.user_id,
        "channels_updated": channel_ids.len(),
    })))
}

// ============================================================================
// 2. CREATE BET (money) — no draw
// ============================================================================
pub async fn create_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    tracing::info!("═══════════════════════════════════════════");
    tracing::info!("💰 CREATE_BET_HANDLER called (pledge)");
    tracing::info!("   fixture_id:        '{}'", payload.fixture_id);
    tracing::info!("   starter_id:        '{}'", payload.starter_id);
    tracing::info!("   starter_name:      '{}'", payload.starter_name);
    tracing::info!("   starter_selection: '{}'", payload.starter_selection);
    tracing::info!("   amount:            {}", payload.amount);
    tracing::info!("   vote_id:           '{:?}'", payload.vote_id);
    tracing::info!("═══════════════════════════════════════════");

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    // No draw.
    let increment_field = match payload.starter_selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        _ => {
            tracing::error!("❌ INVALID starter_selection: '{}'", payload.starter_selection);
            return Err(AppError::ValidationError(
                "Invalid selection. Must be 'home' or 'away'".to_string(),
            ));
        }
    };

    let starter_id = bson::oid::ObjectId::parse_str(&payload.starter_id).map_err(|e| {
        tracing::error!("❌ INVALID starter_id '{}': {}", payload.starter_id, e);
        AppError::InvalidObjectId(e.to_string())
    })?;

    let fixture_id = payload.fixture_id.clone();

    let vote_exists = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &payload.starter_id,
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to check vote existence: {}", e);
            AppError::MongoDB(e)
        })?;

    if let Some(existing_vote) = &vote_exists {
        if existing_vote.selection != payload.starter_selection {
            tracing::warn!(
                "⚠️ VOTE/PLEDGE MISMATCH — user='{}' voted '{}' but tried to bet on '{}'",
                payload.starter_id, existing_vote.selection, payload.starter_selection
            );
            return Err(AppError::ValidationError(format!(
                "You voted for {}, you cannot bet on {}",
                existing_vote.selection, payload.starter_selection
            )));
        }
    }

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    let user = users_col
        .find_one(doc! { "_id": starter_id })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    if user.balance < payload.amount {
        session.abort_transaction().await.map_err(AppError::MongoDB)?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have {}, need {}",
            user.balance, payload.amount
        )));
    }

    if vote_exists.is_none() {
        let vote = Vote::new(
            fixture_id.clone(),
            payload.starter_id.clone(),
            payload.starter_name.clone(),
            payload.starter_selection.clone(),
        );
        votes_col
            .insert_one(&vote)
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;

        let channel_ids = get_user_channel_ids(&channels_col, &payload.starter_id).await?;
        for channel_id in &channel_ids {
            upsert_channel_fixture(
                &channel_fixtures_col,
                channel_id,
                &fixture_id,
                Some(increment_field),
                1,
                "active",
            )
            .await?;
        }
    }

    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    let bet = Bet::new_open(
        fixture_id.clone(),
        payload.starter_id.clone(),
        payload.starter_name.clone(),
        payload.starter_selection.clone(),
        payload.amount,
        payload.vote_id.clone(),
    );

    let insert_result = bets_col
        .insert_one(&bet)
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    let bet_id = insert_result
        .inserted_id
        .as_object_id()
        .map(|oid| oid.to_hex())
        .ok_or_else(|| {
            AppError::InternalServerError("Failed to get bet ID".to_string())
        })?;

    let channel_ids = get_user_channel_ids(&channels_col, &payload.starter_id).await?;
    for channel_id in &channel_ids {
        upsert_channel_fixture(
            &channel_fixtures_col,
            channel_id,
            &fixture_id,
            Some("pledge_count"),
            1,
            "active",
        )
        .await?;
    }

    session.commit_transaction().await.map_err(AppError::MongoDB)?;

    let pledge_payload = serde_json::json!({
        "fixture_id": fixture_id,
        "user_id": payload.starter_id,
        "username": payload.starter_name,
        "selection": payload.starter_selection,
        "amount": payload.amount,
        "bet_id": bet_id,
    });

    for channel_id in &channel_ids {
        let _ = notify_channel_members(
            &state,
            &payload.starter_id,
            channel_id,
            &fixture_id,
            "pledge.create",
            "💰 New Pledge",
            &format!(
                "{} pledged KES {} on the match",
                payload.starter_name, payload.amount
            ),
            pledge_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Pledge created successfully",
        "bet_id": bet_id,
        "vote_id": payload.vote_id,
        "new_balance": user.balance - payload.amount,
        "status": "open",
        "auto_voted": vote_exists.is_none(),
        "channels_updated": channel_ids.len(),
    })))
}

// ============================================================================
// 3. ROLLBACK VOTE — refuses if user has a live poke on the same target
// ============================================================================
pub async fn rollback_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<RollbackVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let pokes_col: Collection<Poke> = state.db.collection("pokes");

    tracing::info!(
        "↩️ Rollback vote: fixture={}, user={}",
        payload.fixture_id,
        payload.user_id
    );

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::ValidationError(
            "User has not voted on this fixture".to_string(),
        ))?;

    // A vote that backs an open/matched poke cannot be rolled back.
    // payload.fixture_id is either a main fixture id or a market_id.
    let live_pokes = pokes_col
        .count_documents(doc! {
            "status": { "$in": ["open", "matched"] },
            "$and": [
                { "$or": [
                    { "starter_id": &payload.user_id },
                    { "finisher_id": &payload.user_id },
                ]},
                { "$or": [
                    { "market_id": &payload.fixture_id },
                    { "market_id": Bson::Null, "fixture_id": &payload.fixture_id },
                ]},
            ],
        })
        .await
        .map_err(AppError::MongoDB)?;

    if live_pokes > 0 {
        return Err(AppError::ValidationError(
            "You have an active poke on this match, you cannot undo your vote".to_string(),
        ));
    }

    votes_col
        .delete_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(AppError::MongoDB)?;

    // Legacy draw votes can still be rolled back even though draw is no
    // longer accepted for new votes.
    let decrement_field = match vote.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => {
            return Err(AppError::ValidationError(
                "Invalid selection in vote record".to_string(),
            ));
        }
    };

    let channel_ids = get_user_channel_ids(&channels_col, &payload.user_id).await?;

    for channel_id in &channel_ids {
        channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": channel_id,
                    "fixture_id": &payload.fixture_id,
                },
                doc! { "$inc": { decrement_field: -1 } },
            )
            .await
            .map_err(AppError::MongoDB)?;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Vote rolled back successfully",
        "fixture_id": payload.fixture_id,
        "user_id": payload.user_id,
        "selection": vote.selection,
        "channels_updated": channel_ids.len(),
    })))
}

// ============================================================================
// 4. FILL BET (money) — no draw
// ============================================================================
pub async fn fill_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<FillBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let now = BsonDateTime::now();

    tracing::info!(
        "🤝 Fill bet: bet={}, finisher={}, amount={}",
        payload.bet_id,
        payload.finisher_id,
        payload.amount
    );

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    // No draw.
    let finisher_increment_field = match payload.finisher_selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        _ => {
            tracing::error!(
                "❌ INVALID finisher_selection: '{}'",
                payload.finisher_selection
            );
            return Err(AppError::ValidationError(
                "Invalid selection. Must be 'home' or 'away'".to_string(),
            ));
        }
    };

    let finisher_id = bson::oid::ObjectId::parse_str(&payload.finisher_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let bet_id = bson::oid::ObjectId::parse_str(&payload.bet_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    let bet = bets_col
        .find_one(doc! { "_id": bet_id, "status": "open" })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::ValidationError(
            "Bet not found or already filled".to_string(),
        ))?;

    if bet.starter_id == payload.finisher_id {
        session.abort_transaction().await.map_err(AppError::MongoDB)?;
        return Err(AppError::ValidationError(
            "Cannot fill your own bet".to_string(),
        ));
    }

    let finisher = users_col
        .find_one(doc! { "_id": finisher_id })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    if finisher.balance < payload.amount {
        session.abort_transaction().await.map_err(AppError::MongoDB)?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have {}, need {}",
            finisher.balance, payload.amount
        )));
    }

    let existing_vote = votes_col
        .find_one(doc! {
            "fixture_id": &bet.fixture_id,
            "user_id": &payload.finisher_id,
        })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    if let Some(v) = &existing_vote {
        if v.selection != payload.finisher_selection {
            session.abort_transaction().await.map_err(AppError::MongoDB)?;
            return Err(AppError::ValidationError(format!(
                "You voted for {}, you cannot bet on {}",
                v.selection, payload.finisher_selection
            )));
        }
    }

    users_col
        .update_one(
            doc! { "_id": finisher_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    bets_col
        .update_one(
            doc! { "_id": bet_id },
            doc! {
                "$set": {
                    "status": "matched",
                    "finisher_id": payload.finisher_id.clone(),
                    "finisher_name": payload.finisher_name.clone(),
                    "finisher_selection": payload.finisher_selection.clone(),
                    "finisher_amount": payload.amount,
                    "matched_at": now,
                }
            },
        )
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    if existing_vote.is_none() {
        let vote = Vote::new(
            bet.fixture_id.clone(),
            payload.finisher_id.clone(),
            payload.finisher_name.clone(),
            payload.finisher_selection.clone(),
        );
        votes_col
            .insert_one(&vote)
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;

        let finisher_channel_ids =
            get_user_channel_ids(&channels_col, &payload.finisher_id).await?;
        for channel_id in &finisher_channel_ids {
            upsert_channel_fixture(
                &channel_fixtures_col,
                channel_id,
                &bet.fixture_id,
                Some(finisher_increment_field),
                1,
                "active",
            )
            .await?;
        }
    }

    let starter_channel_ids = get_user_channel_ids(&channels_col, &bet.starter_id).await?;
    let finisher_channel_ids = get_user_channel_ids(&channels_col, &payload.finisher_id).await?;
    let all_channel_ids: HashSet<String> = starter_channel_ids
        .into_iter()
        .chain(finisher_channel_ids.into_iter())
        .collect();

    for channel_id in &all_channel_ids {
        upsert_channel_fixture(
            &channel_fixtures_col,
            channel_id,
            &bet.fixture_id,
            Some("bet_count"),
            1,
            "active",
        )
        .await?;
    }

    session.commit_transaction().await.map_err(AppError::MongoDB)?;

    let match_payload = serde_json::json!({
        "bet_id": payload.bet_id,
        "fixture_id": bet.fixture_id,
        "starter_id": bet.starter_id,
        "starter_name": bet.starter_name,
        "starter_selection": bet.starter_selection,
        "finisher_id": payload.finisher_id,
        "finisher_name": payload.finisher_name,
        "finisher_selection": payload.finisher_selection,
        "amount": payload.amount,
        "total_pot": bet.starter_amount + payload.amount,
    });

    for channel_id in &all_channel_ids {
        let _ = notify_channel_members(
            &state,
            &payload.finisher_id,
            channel_id,
            &bet.fixture_id,
            "bet.matched",
            "🤝 Bet Matched",
            &format!(
                "{} matched a bet with {} on the match",
                payload.finisher_name, bet.starter_name
            ),
            match_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Bet filled successfully",
        "bet_id": payload.bet_id,
        "status": "matched",
        "total_pot": bet.starter_amount + payload.amount,
        "channels_updated": all_channel_ids.len(),
    })))
}

// ============================================================================
// 5. SETTLE BETS (money) + vote scoring — draw refunds
// ============================================================================
pub async fn settle_bets_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettleBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("games");
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let now = BsonDateTime::now();

    tracing::info!(
        "⚖️ Settling bets: fixture={}, result={}",
        payload.fixture_id,
        payload.result
    );

    let mut settled_count = 0;
    let mut refund_count = 0;

    let mut channel_cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.fixture_id })
        .await
        .map_err(AppError::MongoDB)?;

    let mut all_channel_ids = HashSet::new();
    while let Some(cf) = channel_cursor.next().await {
        let cf: ChannelFixture = cf.map_err(AppError::MongoDB)?;
        all_channel_ids.insert(cf.channel_id);
    }

    // ---- matched bets ----
    let mut matched_cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "matched",
        })
        .await
        .map_err(AppError::MongoDB)?;

    let mut refunded_bets: Vec<(String, String)> = Vec::new();

    while let Some(bet) = matched_cursor.next().await {
        let bet: Bet = bet.map_err(AppError::MongoDB)?;

        let (starter_won, finisher_won) = match payload.result.as_str() {
            "home" => (
                bet.starter_selection == "home",
                bet.finisher_selection.as_deref() == Some("home"),
            ),
            "away" => (
                bet.starter_selection == "away",
                bet.finisher_selection.as_deref() == Some("away"),
            ),
            "draw" => (
                bet.starter_selection == "draw",
                bet.finisher_selection.as_deref() == Some("draw"),
            ),
            _ => (false, false),
        };

        let total_pot = bet.starter_amount + bet.finisher_amount.unwrap_or(0.0);
        let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
        session.start_transaction().await.map_err(AppError::MongoDB)?;

        let bet_id = bet.id.ok_or(AppError::DocumentNotFound)?;

        if starter_won && !finisher_won {
            let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id)
                .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

            users_col
                .update_one(
                    doc! { "_id": starter_oid },
                    doc! { "$inc": { "balance": total_pot } },
                )
                .session(&mut session)
                .await
                .map_err(AppError::MongoDB)?;

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! { "$set": {
                        "status": "settled",
                        "winner_id": &bet.starter_id,
                        "starter_result": "won",
                        "finisher_result": "lost",
                        "settled_at": now,
                    }},
                )
                .session(&mut session)
                .await
                .map_err(AppError::MongoDB)?;
        } else if finisher_won && !starter_won {
            if let Some(finisher_id) = &bet.finisher_id {
                let finisher_oid = bson::oid::ObjectId::parse_str(finisher_id)
                    .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
                users_col
                    .update_one(
                        doc! { "_id": finisher_oid },
                        doc! { "$inc": { "balance": total_pot } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(AppError::MongoDB)?;
            }

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! { "$set": {
                        "status": "settled",
                        "winner_id": &bet.finisher_id,
                        "starter_result": "lost",
                        "finisher_result": "won",
                        "settled_at": now,
                    }},
                )
                .session(&mut session)
                .await
                .map_err(AppError::MongoDB)?;
        } else {
            // Draw / no winner -> refund both.
            let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id)
                .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
            users_col
                .update_one(
                    doc! { "_id": starter_oid },
                    doc! { "$inc": { "balance": bet.starter_amount } },
                )
                .session(&mut session)
                .await
                .map_err(AppError::MongoDB)?;

            if let Some(finisher_id) = &bet.finisher_id {
                let finisher_oid = bson::oid::ObjectId::parse_str(finisher_id)
                    .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
                users_col
                    .update_one(
                        doc! { "_id": finisher_oid },
                        doc! { "$inc": { "balance": bet.finisher_amount.unwrap_or(0.0) } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(AppError::MongoDB)?;
            }

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! { "$set": {
                        "status": "refunded",
                        "starter_result": "draw",
                        "finisher_result": "draw",
                        "settled_at": now,
                    }},
                )
                .session(&mut session)
                .await
                .map_err(AppError::MongoDB)?;

            refunded_bets.push((bet_id.to_hex(), bet.starter_id.clone()));
        }

        session.commit_transaction().await.map_err(AppError::MongoDB)?;
        settled_count += 1;
    }

    // ---- unmatched bets ----
    let mut open_cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "open",
        })
        .await
        .map_err(AppError::MongoDB)?;

    while let Some(bet) = open_cursor.next().await {
        let bet: Bet = bet.map_err(AppError::MongoDB)?;

        let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
        session.start_transaction().await.map_err(AppError::MongoDB)?;

        let bet_id = bet.id.ok_or(AppError::DocumentNotFound)?;

        let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id)
            .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
        users_col
            .update_one(
                doc! { "_id": starter_oid },
                doc! { "$inc": { "balance": bet.starter_amount } },
            )
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;

        bets_col
            .update_one(
                doc! { "_id": bet_id },
                doc! { "$set": {
                    "status": "refunded",
                    "starter_result": "unmatched",
                    "settled_at": now,
                }},
            )
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;

        session.commit_transaction().await.map_err(AppError::MongoDB)?;
        refund_count += 1;

        refunded_bets.push((bet_id.to_hex(), bet.starter_id.clone()));
    }

    // ---- mark fixture / game completed ----
    games_col
        .update_one(
            doc! { "matchId": &payload.fixture_id },
            doc! { "$set": {
                "status": "completed",
                "result": &payload.result,
                "settled_at": now,
            }},
        )
        .await
        .map_err(AppError::MongoDB)?;

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await
        .map_err(AppError::MongoDB)?;

    // ---- mark votes correct/incorrect ----
    votes_col
        .update_many(
            doc! {
                "fixture_id": &payload.fixture_id,
                "selection": &payload.result,
                "is_correct": null,
            },
            doc! { "$set": { "is_correct": true, "points_awarded": 3 } },
        )
        .await
        .map_err(AppError::MongoDB)?;

    votes_col
        .update_many(
            doc! {
                "fixture_id": &payload.fixture_id,
                "selection": { "$ne": &payload.result },
                "is_correct": null,
            },
            doc! { "$set": { "is_correct": false, "points_awarded": -3 } },
        )
        .await
        .map_err(AppError::MongoDB)?;

    // ---- award/deduct season points on channels + users ----
    let mut correct_cursor = votes_col
        .find(doc! { "fixture_id": &payload.fixture_id, "is_correct": true })
        .await
        .map_err(AppError::MongoDB)?;
    let mut correct_ids: Vec<String> = Vec::new();
    while let Some(v) = correct_cursor.next().await {
        if let Ok(v) = v {
            correct_ids.push(v.user_id);
        }
    }

    let mut incorrect_cursor = votes_col
        .find(doc! { "fixture_id": &payload.fixture_id, "is_correct": false })
        .await
        .map_err(AppError::MongoDB)?;
    let mut incorrect_ids: Vec<String> = Vec::new();
    while let Some(v) = incorrect_cursor.next().await {
        if let Ok(v) = v {
            incorrect_ids.push(v.user_id);
        }
    }

    if !correct_ids.is_empty() {
        channels_col
            .update_many(
                doc! { "members.user_id": { "$in": &correct_ids } },
                doc! { "$inc": {
                    "members.$[m].season_points": 3,
                    "members.$[m].correct_votes": 1,
                    "members.$[m].total_votes": 1,
                }},
            )
            .array_filters(vec![doc! { "m.user_id": { "$in": &correct_ids } }])
            .await
            .map_err(AppError::MongoDB)?;

        let correct_oids: Vec<bson::oid::ObjectId> = correct_ids
            .iter()
            .filter_map(|id| bson::oid::ObjectId::parse_str(id).ok())
            .collect();
        if !correct_oids.is_empty() {
            users_col
                .update_many(
                    doc! { "_id": { "$in": &correct_oids } },
                    doc! { "$inc": {
                        "season_points": 3,
                        "correct_votes": 1,
                        "total_votes": 1,
                    }},
                )
                .await
                .map_err(AppError::MongoDB)?;
        }
    }

    if !incorrect_ids.is_empty() {
        channels_col
            .update_many(
                doc! { "members.user_id": { "$in": &incorrect_ids } },
                doc! { "$inc": {
                    "members.$[m].season_points": -3,
                    "members.$[m].total_votes": 1,
                }},
            )
            .array_filters(vec![doc! { "m.user_id": { "$in": &incorrect_ids } }])
            .await
            .map_err(AppError::MongoDB)?;

        let incorrect_oids: Vec<bson::oid::ObjectId> = incorrect_ids
            .iter()
            .filter_map(|id| bson::oid::ObjectId::parse_str(id).ok())
            .collect();
        if !incorrect_oids.is_empty() {
            users_col
                .update_many(
                    doc! { "_id": { "$in": &incorrect_oids } },
                    doc! { "$inc": {
                        "season_points": -3,
                        "total_votes": 1,
                    }},
                )
                .await
                .map_err(AppError::MongoDB)?;
        }
    }

    let result_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "result": payload.result,
        "settled_count": settled_count,
        "refunded_count": refund_count,
        "correct_votes": correct_ids.len(),
        "incorrect_votes": incorrect_ids.len(),
    });

    for channel_id in &all_channel_ids {
        let _ = notify_channel_members(
            &state,
            "system",
            channel_id,
            &payload.fixture_id,
            "bet.settled",
            "⚖️ Match Settled",
            &format!(
                "Match result: {}. {} bets settled.",
                payload.result, settled_count
            ),
            result_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Settled {} bets, refunded {} unmatched bets", settled_count, refund_count),
        "settled": settled_count,
        "refunded": refund_count,
        "votes_correct": correct_ids.len(),
        "votes_incorrect": incorrect_ids.len(),
    })))
}

// ============================================================================
// 6. GET FIXTURE VOTERS
// ============================================================================
pub async fn get_fixture_voters_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(AppError::MongoDB)?;

    let mut voters = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(AppError::MongoDB)?;
        voters.push(json!({
            "user_id": vote.user_id,
            "user_name": vote.user_name,
            "selection": vote.selection,
            "voted_at": vote.voted_at,
            "is_correct": vote.is_correct,
            "points_awarded": vote.points_awarded,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "voters": voters,
        "total_votes": voters.len(),
    })))
}

// ============================================================================
// 7. GET USER VOTES
// ============================================================================
pub async fn get_user_votes_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let mut cursor = votes_col
        .find(doc! { "user_id": &user_id })
        .await
        .map_err(AppError::MongoDB)?;

    let mut votes = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(AppError::MongoDB)?;
        votes.push(json!({
            "fixture_id": vote.fixture_id,
            "selection": vote.selection,
            "voted_at": vote.voted_at,
            "is_correct": vote.is_correct,
            "points_awarded": vote.points_awarded,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "votes": votes,
        "count": votes.len(),
    })))
}

// ============================================================================
// 8. CHECK USER VOTE
// ============================================================================
pub async fn check_user_vote_handler(
    State(state): State<AppState>,
    Path((fixture_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &user_id,
        })
        .await
        .map_err(AppError::MongoDB)?;

    Ok(Json(json!({
        "success": true,
        "has_voted": vote.is_some(),
        "selection": vote.as_ref().map(|v| v.selection.clone()),
        "is_correct": vote.as_ref().and_then(|v| v.is_correct),
        "points_awarded": vote.as_ref().and_then(|v| v.points_awarded),
    })))
}

// ============================================================================
// 9. GET CHANNEL VOTE COUNT
// ============================================================================
pub async fn get_channel_vote_count_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");

    let cf = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    let total_votes = cf.vote_counts.home + cf.vote_counts.away + cf.vote_counts.draw;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "vote_count": total_votes,
        "vote_counts": {
            "home": cf.vote_counts.home,
            "away": cf.vote_counts.away,
            "draw": cf.vote_counts.draw,
        },
    })))
}

// ============================================================================
// 10. GET VOTE BREAKDOWN
// ============================================================================
pub async fn get_vote_breakdown_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let pipeline = vec![
        doc! { "$match": { "fixture_id": &fixture_id } },
        doc! { "$group": { "_id": "$selection", "count": { "$sum": 1 } } },
    ];

    let mut cursor = votes_col.aggregate(pipeline).await.map_err(AppError::MongoDB)?;

    let mut breakdown = serde_json::Map::new();
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(AppError::MongoDB)?;
        if let (Some(selection), Some(count)) = (
            doc.get("_id").and_then(|v| v.as_str()),
            doc.get("count").and_then(|v| v.as_i64()),
        ) {
            breakdown.insert(selection.to_string(), json!(count));
        }
    }

    let home = breakdown.get("home").and_then(|v| v.as_i64()).unwrap_or(0);
    let away = breakdown.get("away").and_then(|v| v.as_i64()).unwrap_or(0);
    let draw = breakdown.get("draw").and_then(|v| v.as_i64()).unwrap_or(0);

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "breakdown": { "home": home, "away": away, "draw": draw },
        "total": home + away + draw,
    })))
}

// ============================================================================
// 11. GET CHANNEL VOTES
// ============================================================================
pub async fn get_channel_votes_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(AppError::MongoDB)?;

    let mut channel_votes = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(AppError::MongoDB)?;
        if member_ids.contains(&vote.user_id) {
            channel_votes.push(json!({
                "user_id": vote.user_id,
                "user_name": vote.user_name,
                "selection": vote.selection,
                "voted_at": vote.voted_at,
                "is_correct": vote.is_correct,
                "points_awarded": vote.points_awarded,
            }));
        }
    }

    let vote_count = channel_votes.len();

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "votes": channel_votes,
        "count": vote_count,
        "vote_count": vote_count,
    })))
}

// ============================================================================
// 12. GET CHANNEL PLEDGES
// ============================================================================
pub async fn get_channel_pledges_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut cursor = bets_col
        .find(doc! { "fixture_id": &fixture_id, "status": "open" })
        .await
        .map_err(AppError::MongoDB)?;

    let mut open_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(AppError::MongoDB)?;
        if member_ids.contains(&bet.starter_id) {
            open_bets.push(json!({
                "id": bet.id.map(|oid| oid.to_hex()),
                "starter_id": bet.starter_id,
                "starter_name": bet.starter_name,
                "starter_selection": bet.starter_selection,
                "starter_amount": bet.starter_amount,
                "status": bet.status,
                "created_at": bet.created_at,
            }));
        }
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "pledges": open_bets,
        "count": open_bets.len(),
    })))
}

// ============================================================================
// 13. GET CHANNEL BETTORS (matched + settled)
// ============================================================================
pub async fn get_channel_bettors_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": { "$in": vec!["matched", "settled"] },
        })
        .await
        .map_err(AppError::MongoDB)?;

    let mut matched_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(AppError::MongoDB)?;
        if let Some(finisher_id) = &bet.finisher_id {
            if member_ids.contains(&bet.starter_id) && member_ids.contains(finisher_id) {
                matched_bets.push(json!({
                    "id": bet.id.map(|oid| oid.to_hex()),
                    "starter_id": bet.starter_id,
                    "starter_name": bet.starter_name,
                    "starter_selection": bet.starter_selection,
                    "starter_amount": bet.starter_amount,
                    "finisher_id": bet.finisher_id,
                    "finisher_name": bet.finisher_name,
                    "finisher_selection": bet.finisher_selection,
                    "finisher_amount": bet.finisher_amount,
                    "status": bet.status,
                    "total_pot": bet.total_pot(),
                    "created_at": bet.created_at,
                    "matched_at": bet.matched_at,
                }));
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "bettors": matched_bets,
        "count": matched_bets.len(),
    })))
}

// ============================================================================
// 14. GET CHANNEL MEMBERS
// ============================================================================
pub async fn get_channel_members_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: Vec<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "member_ids": member_ids,
        "count": member_ids.len(),
    })))
}

// ============================================================================
// 15. GET USER'S BETS
// ============================================================================
pub async fn get_user_bets_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");

    let mut cursor = bets_col
        .find(doc! {
            "$or": [
                { "starter_id": &user_id },
                { "finisher_id": &user_id },
            ]
        })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(AppError::MongoDB)?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(AppError::MongoDB)?;
        bets.push(json!({
            "id": bet.id.map(|oid| oid.to_hex()),
            "fixture_id": bet.fixture_id,
            "starter_id": bet.starter_id,
            "starter_name": bet.starter_name,
            "starter_selection": bet.starter_selection,
            "starter_amount": bet.starter_amount,
            "finisher_id": bet.finisher_id,
            "finisher_name": bet.finisher_name,
            "finisher_selection": bet.finisher_selection,
            "finisher_amount": bet.finisher_amount,
            "vote_id": bet.vote_id,
            "status": bet.status,
            "winner_id": bet.winner_id,
            "starter_result": bet.starter_result,
            "finisher_result": bet.finisher_result,
            "created_at": bet.created_at,
            "matched_at": bet.matched_at,
            "settled_at": bet.settled_at,
            "total_pot": bet.total_pot(),
        }));
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "bets": bets,
        "count": bets.len(),
    })))
}

// ============================================================================
// 16. CREATE POKE (points) — atomic with auto-vote, no draw
// ============================================================================
pub async fn create_poke_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreatePokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let users_doc_col: Collection<Document> = state.db.collection("users");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    tracing::info!(
        "👉 CREATE_POKE: fixture='{}', market='{:?}', starter='{}', selection='{}', stake={}",
        payload.fixture_id,
        payload.market_id,
        payload.starter_id,
        payload.starter_selection,
        payload.stake
    );

    if payload.stake <= 0 {
        return Err(AppError::ValidationError(
            "Stake must be greater than 0".to_string(),
        ));
    }

    // Validate target + selection (no draw, market open if sub-fixture).
    let target = load_poke_target(&state, &payload.fixture_id, &payload.market_id, true).await?;
    check_selection(&payload.starter_selection, &target.options)?;

    let starter_oid = bson::oid::ObjectId::parse_str(&payload.starter_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    let poke = Poke::new_open(
        payload.fixture_id.clone(),
        target.market_id.clone(),
        payload.starter_id.clone(),
        payload.starter_name.clone(),
        payload.starter_selection.clone(),
        payload.stake,
    );
    let vote_key = poke.vote_key();

    let existing_vote = votes_col
        .find_one(doc! { "fixture_id": &vote_key, "user_id": &payload.starter_id })
        .await
        .map_err(AppError::MongoDB)?;
    let needs_auto_vote = check_vote_matches(&existing_vote, &payload.starter_selection)?;

    let channel_ids = get_user_channel_ids(&channels_col, &payload.starter_id).await?;

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    let user = users_doc_col
        .find_one(doc! { "_id": starter_oid })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;
    let points = read_points(&user);

    if points < MIN_POKE_POINTS as i64 {
        return Err(AppError::ValidationError(format!(
            "You need at least {} points to poke. You have {}",
            MIN_POKE_POINTS, points
        )));
    }
    if points < payload.stake as i64 {
        return Err(AppError::ValidationError(format!(
            "Insufficient points. You have {}, need {}",
            points, payload.stake
        )));
    }

    if needs_auto_vote {
        let vote = Vote::new(
            vote_key.clone(),
            payload.starter_id.clone(),
            payload.starter_name.clone(),
            payload.starter_selection.clone(),
        );
        votes_col
            .insert_one(&vote)
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;
    }

    reserve_points(
        &users_doc_col,
        &channels_col,
        starter_oid,
        &payload.starter_id,
        payload.stake,
        &mut session,
    )
    .await?;

    let insert_result = pokes_col
        .insert_one(&poke)
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;

    let poke_id = insert_result
        .inserted_id
        .as_object_id()
        .map(|oid| oid.to_hex())
        .ok_or_else(|| AppError::InternalServerError("Failed to get poke ID".to_string()))?;

    session.commit_transaction().await.map_err(AppError::MongoDB)?;

    // Counter updates post-commit (best-effort).
    let main_vote_field = if target.market_id.is_none() {
        vote_count_field(&payload.starter_selection)
    } else {
        None
    };

    for channel_id in &channel_ids {
        if needs_auto_vote {
            if let Some(field) = main_vote_field {
                if let Err(e) = upsert_channel_fixture(
                    &channel_fixtures_col,
                    channel_id,
                    &payload.fixture_id,
                    Some(field),
                    1,
                    "active",
                )
                .await
                {
                    tracing::warn!("⚠️ vote_counts update failed for '{}': {:?}", channel_id, e);
                }
            }
        }
        if let Err(e) = upsert_channel_fixture(
            &channel_fixtures_col,
            channel_id,
            &payload.fixture_id,
            Some("poke_count"),
            1,
            "active",
        )
        .await
        {
            tracing::warn!("⚠️ poke_count update failed for '{}': {:?}", channel_id, e);
        }
    }

    let poke_payload = json!({
        "poke_id": poke_id,
        "fixture_id": payload.fixture_id,
        "market_id": target.market_id,
        "user_id": payload.starter_id,
        "username": payload.starter_name,
        "selection": payload.starter_selection,
        "stake": payload.stake,
    });

    let target_label = if target.market_id.is_some() {
        "a sub-fixture"
    } else {
        "the match"
    };

    for channel_id in &channel_ids {
        let _ = notify_channel_members(
            &state,
            &payload.starter_id,
            channel_id,
            &payload.fixture_id,
            "poke.create",
            "👉 New Poke",
            &format!(
                "{} poked {} points on {}",
                payload.starter_name, payload.stake, target_label
            ),
            poke_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Poke created successfully",
        "poke_id": poke_id,
        "market_id": target.market_id,
        "status": "open",
        "stake": payload.stake,
        "new_points": points - payload.stake as i64,
        "auto_voted": needs_auto_vote,
        "channels_updated": channel_ids.len(),
    })))
}

// ============================================================================
// 17. FILL POKE (points) — equal stake, different side, shared channel
// ============================================================================
pub async fn fill_poke_handler(
    State(state): State<AppState>,
    Json(payload): Json<FillPokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let users_doc_col: Collection<Document> = state.db.collection("users");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let now = BsonDateTime::now();

    let finisher_oid = bson::oid::ObjectId::parse_str(&payload.finisher_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let poke_oid = bson::oid::ObjectId::parse_str(&payload.poke_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    // 1. Poke must be OPEN.
    let poke = pokes_col
        .find_one(doc! { "_id": poke_oid, "status": "open" })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::ValidationError(
            "Poke not found or already filled".to_string(),
        ))?;

    // 2. Target still valid + selection allowed (no draw).
    let target = load_poke_target(&state, &poke.fixture_id, &poke.market_id, true).await?;
    check_selection(&payload.finisher_selection, &target.options)?;

    // 3. Own poke / same side.
    if poke.starter_id == payload.finisher_id {
        return Err(AppError::ValidationError(
            "Cannot fill your own poke".to_string(),
        ));
    }
    if poke.starter_selection == payload.finisher_selection {
        return Err(AppError::ValidationError(
            "You must pick a different outcome from the poke starter".to_string(),
        ));
    }

    // 4. Must share a channel with the starter.
    let starter_channels = get_user_channel_ids(&channels_col, &poke.starter_id).await?;
    let finisher_channels = get_user_channel_ids(&channels_col, &payload.finisher_id).await?;
    let finisher_set: HashSet<&String> = finisher_channels.iter().collect();
    let shared_channels: Vec<String> = starter_channels
        .iter()
        .filter(|c| finisher_set.contains(c))
        .cloned()
        .collect();
    if shared_channels.is_empty() {
        return Err(AppError::ValidationError(
            "You can only fill pokes from members of your channels".to_string(),
        ));
    }

    // 5. Points check.
    let finisher = users_doc_col
        .find_one(doc! { "_id": finisher_oid })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;
    let points = read_points(&finisher);

    if points < MIN_POKE_POINTS as i64 {
        return Err(AppError::ValidationError(format!(
            "You need at least {} points to poke. You have {}",
            MIN_POKE_POINTS, points
        )));
    }
    if points < poke.stake as i64 {
        return Err(AppError::ValidationError(format!(
            "Insufficient points. You have {}, need {}",
            points, poke.stake
        )));
    }

    // 6. Vote consistency.
    let vote_key = poke.vote_key();
    let existing_vote = votes_col
        .find_one(doc! { "fixture_id": &vote_key, "user_id": &payload.finisher_id })
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;
    let needs_auto_vote = check_vote_matches(&existing_vote, &payload.finisher_selection)?;

    // 7. Reserve points (atomic $gte guard).
    reserve_points(
        &users_doc_col,
        &channels_col,
        finisher_oid,
        &payload.finisher_id,
        poke.stake,
        &mut session,
    )
    .await?;

    // 8. open -> matched (status filter blocks double fill).
    let upd = pokes_col
        .update_one(
            doc! { "_id": poke_oid, "status": "open" },
            doc! { "$set": {
                "status": "matched",
                "finisher_id": &payload.finisher_id,
                "finisher_name": &payload.finisher_name,
                "finisher_selection": &payload.finisher_selection,
                "matched_at": now,
            }},
        )
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;
    if upd.matched_count == 0 {
        return Err(AppError::ValidationError("Poke already filled".to_string()));
    }

    // 9. Auto-vote.
    if needs_auto_vote {
        let vote = Vote::new(
            vote_key.clone(),
            payload.finisher_id.clone(),
            payload.finisher_name.clone(),
            payload.finisher_selection.clone(),
        );
        votes_col
            .insert_one(&vote)
            .session(&mut session)
            .await
            .map_err(AppError::MongoDB)?;
    }

    session.commit_transaction().await.map_err(AppError::MongoDB)?;

    // vote_counts only exist for main-match votes.
    if needs_auto_vote && poke.market_id.is_none() {
        if let Some(field) = vote_count_field(&payload.finisher_selection) {
            for channel_id in &finisher_channels {
                if let Err(e) = upsert_channel_fixture(
                    &channel_fixtures_col,
                    channel_id,
                    &poke.fixture_id,
                    Some(field),
                    1,
                    "active",
                )
                .await
                {
                    tracing::warn!("⚠️ vote_counts update failed for '{}': {:?}", channel_id, e);
                }
            }
        }
    }

    let match_payload = json!({
        "poke_id": payload.poke_id,
        "fixture_id": poke.fixture_id,
        "market_id": poke.market_id,
        "starter_id": poke.starter_id,
        "starter_name": poke.starter_name,
        "starter_selection": poke.starter_selection,
        "finisher_id": payload.finisher_id,
        "finisher_name": payload.finisher_name,
        "finisher_selection": payload.finisher_selection,
        "stake": poke.stake,
        "total_pot": poke.stake * 2,
    });

    for channel_id in &shared_channels {
        let _ = notify_channel_members(
            &state,
            &payload.finisher_id,
            channel_id,
            &poke.fixture_id,
            "poke.matched",
            "🤝 Poke Matched",
            &format!(
                "{} matched {}'s poke",
                payload.finisher_name, poke.starter_name
            ),
            match_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Poke filled successfully",
        "poke_id": payload.poke_id,
        "status": "matched",
        "stake": poke.stake,
        "total_pot": poke.stake * 2,
        "new_points": points - poke.stake as i64,
        "auto_voted": needs_auto_vote,
    })))
}

// ============================================================================
// 18. SETTLE POKES — exactly-once per poke, per-poke transaction
// ============================================================================

/// `winner`: winning selection, or `None` to refund both sides (draw / no winner).
async fn settle_one_matched_poke(
    state: &AppState,
    poke: &Poke,
    winner: Option<&str>,
) -> Result<&'static str, AppError> {
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let users_doc_col: Collection<Document> = state.db.collection("users");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let now = BsonDateTime::now();

    let poke_oid = poke.id.ok_or(AppError::DocumentNotFound)?;
    let finisher_id = poke.finisher_id.clone().ok_or(AppError::DocumentNotFound)?;
    let starter_won = winner == Some(poke.starter_selection.as_str());
    let finisher_won = winner.is_some() && winner == poke.finisher_selection.as_deref();
    let pot = poke.stake * 2;

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    let (update_doc, outcome) = if starter_won {
        (doc! { "$set": {
            "status": "settled",
            "winner_id": &poke.starter_id,
            "starter_result": "won",
            "finisher_result": "lost",
            "settled_at": now,
        }}, "settled")
    } else if finisher_won {
        (doc! { "$set": {
            "status": "settled",
            "winner_id": &finisher_id,
            "starter_result": "lost",
            "finisher_result": "won",
            "settled_at": now,
        }}, "settled")
    } else {
        (doc! { "$set": {
            "status": "refunded",
            "starter_result": "refunded",
            "finisher_result": "refunded",
            "settled_at": now,
        }}, "refunded")
    };

    // Only flips if still "matched" => can never pay out twice.
    let upd = pokes_col
        .update_one(doc! { "_id": poke_oid, "status": "matched" }, update_doc)
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;
    if upd.matched_count == 0 {
        return Ok("skipped");
    }

    if outcome == "settled" {
        let winner_id = if starter_won { &poke.starter_id } else { &finisher_id };
        credit_points(&users_doc_col, &channels_col, winner_id, pot, &mut session).await?;
    } else {
        credit_points(&users_doc_col, &channels_col, &poke.starter_id, poke.stake, &mut session)
            .await?;
        credit_points(&users_doc_col, &channels_col, &finisher_id, poke.stake, &mut session)
            .await?;
    }

    session.commit_transaction().await.map_err(AppError::MongoDB)?;
    Ok(outcome)
}

async fn refund_one_open_poke(state: &AppState, poke: &Poke) -> Result<bool, AppError> {
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let users_doc_col: Collection<Document> = state.db.collection("users");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let poke_oid = poke.id.ok_or(AppError::DocumentNotFound)?;

    let mut session = state.client.start_session().await.map_err(AppError::MongoDB)?;
    session.start_transaction().await.map_err(AppError::MongoDB)?;

    let upd = pokes_col
        .update_one(
            doc! { "_id": poke_oid, "status": "open" },
            doc! { "$set": {
                "status": "refunded",
                "starter_result": "unmatched",
                "settled_at": BsonDateTime::now(),
            }},
        )
        .session(&mut session)
        .await
        .map_err(AppError::MongoDB)?;
    if upd.matched_count == 0 {
        return Ok(false);
    }

    credit_points(&users_doc_col, &channels_col, &poke.starter_id, poke.stake, &mut session).await?;
    session.commit_transaction().await.map_err(AppError::MongoDB)?;
    Ok(true)
}

pub async fn settle_pokes_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettlePokeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");

    let target = load_poke_target(&state, &payload.fixture_id, &payload.market_id, false).await?;

    // `None` / `"draw"` (main match only) => refund all. Otherwise the result
    // must be a valid option for this target.
    let winner: Option<String> = match payload.result.as_deref() {
        None => None,
        Some("draw") if target.market_id.is_none() => None,
        Some(r) => {
            check_selection(r, &target.options)?;
            Some(r.to_string())
        }
    };

    tracing::info!(
        "⚖️ Settling pokes: fixture={}, market={:?}, winner={:?}",
        payload.fixture_id,
        target.market_id,
        winner
    );

    let mut all_channel_ids = HashSet::new();
    let mut cf_cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.fixture_id })
        .await
        .map_err(AppError::MongoDB)?;
    while let Some(cf) = cf_cursor.next().await {
        if let Ok(cf) = cf {
            all_channel_ids.insert(cf.channel_id);
        }
    }

    let mut settled = 0;
    let mut refunded = 0;
    let mut failed = 0;

    // matched pokes
    let mut matched_filter = poke_target_filter(&payload.fixture_id, &target.market_id);
    matched_filter.insert("status", "matched");
    let mut cursor = pokes_col.find(matched_filter).await.map_err(AppError::MongoDB)?;
    let mut matched: Vec<Poke> = Vec::new();
    while let Some(p) = cursor.next().await {
        matched.push(p.map_err(AppError::MongoDB)?);
    }
    for poke in &matched {
        match settle_one_matched_poke(&state, poke, winner.as_deref()).await {
            Ok("settled") => settled += 1,
            Ok("refunded") => refunded += 1,
            Ok(_) => {}
            Err(e) => {
                failed += 1;
                tracing::error!("❌ Failed to settle poke {:?}: {:?}", poke.id, e);
            }
        }
    }

    // unmatched pokes: refund
    let mut open_filter = poke_target_filter(&payload.fixture_id, &target.market_id);
    open_filter.insert("status", "open");
    let mut cursor = pokes_col.find(open_filter).await.map_err(AppError::MongoDB)?;
    let mut open: Vec<Poke> = Vec::new();
    while let Some(p) = cursor.next().await {
        open.push(p.map_err(AppError::MongoDB)?);
    }
    let mut unmatched_refunded = 0;
    for poke in &open {
        match refund_one_open_poke(&state, poke).await {
            Ok(true) => unmatched_refunded += 1,
            Ok(false) => {}
            Err(e) => {
                failed += 1;
                tracing::error!("❌ Failed to refund poke {:?}: {:?}", poke.id, e);
            }
        }
    }

    let result_payload = json!({
        "fixture_id": payload.fixture_id,
        "market_id": target.market_id,
        "winner": winner,
        "settled_count": settled,
        "refunded_count": refunded + unmatched_refunded,
    });

    for channel_id in &all_channel_ids {
        let _ = notify_channel_members(
            &state,
            "system",
            channel_id,
            &payload.fixture_id,
            "poke.settled",
            "⚖️ Pokes Settled",
            &format!("{} pokes settled.", settled),
            result_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": failed == 0,
        "message": format!("Settled {} pokes, refunded {}", settled, refunded + unmatched_refunded),
        "settled": settled,
        "refunded_no_winner": refunded,
        "refunded_unmatched": unmatched_refunded,
        "failed": failed,
    })))
}

// ============================================================================
// 19. GET USER POINTS
// ============================================================================
pub async fn get_user_points_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let users_doc_col: Collection<Document> = state.db.collection("users");
    let oid = bson::oid::ObjectId::parse_str(&user_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    let user = users_doc_col
        .find_one(doc! { "_id": oid })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;
    let points = read_points(&user);

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "points": points,
        "min_points_to_poke": MIN_POKE_POINTS,
        "can_poke": points >= MIN_POKE_POINTS as i64,
    })))
}

// ============================================================================
// 20. GET USER POKES
// ============================================================================
pub async fn get_user_pokes_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pokes_col: Collection<Poke> = state.db.collection("pokes");

    let mut cursor = pokes_col
        .find(doc! { "$or": [{ "starter_id": &user_id }, { "finisher_id": &user_id }] })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(AppError::MongoDB)?;

    let mut pokes = Vec::new();
    while let Some(p) = cursor.next().await {
        pokes.push(poke_json(&p.map_err(AppError::MongoDB)?));
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "pokes": pokes,
        "count": pokes.len(),
    })))
}

// ============================================================================
// 21. GET CHANNEL POKES — open + matched in one call
//     Query: ?market_id=...   (omit for main match)
// ============================================================================
pub async fn get_channel_pokes_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
    Query(query): Query<PokeListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pokes_col: Collection<Poke> = state.db.collection("pokes");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(AppError::MongoDB)?
        .ok_or(AppError::DocumentNotFound)?;
    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut filter = poke_target_filter(&fixture_id, &query.market_id);
    filter.insert("status", doc! { "$in": ["open", "matched", "settled"] });

    let mut cursor = pokes_col
        .find(filter)
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(AppError::MongoDB)?;

    let mut open = Vec::new();
    let mut matched = Vec::new();
    while let Some(p) = cursor.next().await {
        let p = p.map_err(AppError::MongoDB)?;
        match p.status.as_str() {
            "open" if member_ids.contains(&p.starter_id) => open.push(poke_json(&p)),
            "matched" | "settled" => {
                if let Some(f) = &p.finisher_id {
                    if member_ids.contains(&p.starter_id) && member_ids.contains(f) {
                        matched.push(poke_json(&p));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "market_id": query.market_id,
        "channel_id": channel_id,
        "open": open,
        "matched": matched,
        "open_count": open.len(),
        "matched_count": matched.len(),
    })))
}