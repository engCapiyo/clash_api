use crate::{
    errors::AppError,
    models::{
        channel::Channel,
        game::Game,
        sub_fixture::{
            BetStatus, CreateSubFixtureBetRequest, CreateSubFixtureMarketRequest,
            FillSubFixtureBetRequest, SettleSubFixtureMarketRequest, SubFixtureBet,
            SubFixtureBetResponse, SubFixtureMarket, SubFixtureMarketResponse,
        },
        user::User,
    },
    AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use bson::{doc, DateTime as BsonDateTime};
use futures_util::StreamExt;
use mongodb::Collection;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ============================================================================
// HELPER: Get channel members
// ============================================================================
async fn get_channel_members_for_match(
    channels_col: &Collection<Channel>,
    match_id: &str,
) -> Result<Vec<String>, AppError> {
    let mut cursor = channels_col
        .find(doc! { "fixtures": match_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channels for match: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut all_members = HashSet::new();
    while let Some(channel) = cursor.next().await {
        let channel: Channel = channel.map_err(|e| {
            tracing::error!("❌ Failed to deserialize channel: {}", e);
            AppError::MongoDB(e)
        })?;
        for member in channel.members {
            all_members.insert(member.user_id);
        }
    }

    Ok(all_members.into_iter().collect())
}

// ============================================================================
// HELPER: Send WebSocket broadcast
// ============================================================================
async fn broadcast_to_match_room(
    state: &AppState,
    match_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) {
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let mut cursor = match channels_col.find(doc! { "fixtures": match_id }).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("❌ Failed to find channels for broadcast: {}", e);
            return;
        }
    };

    // ✅ FIX: cursor.next() returns Option<Result<Channel>>
    while let Some(channel_result) = cursor.next().await {
        let channel: Channel = match channel_result {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("❌ Failed to deserialize channel: {}", e);
                continue;
            }
        };

        let room_key = format!("{}_{}", channel.channel_id, match_id);
        let tx = state.get_or_create_broadcaster(&room_key);

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
}

// ============================================================================
// HELPER: Send notification to channel members
// ============================================================================
async fn notify_sub_fixture_members(
    state: &AppState,
    match_id: &str,
    actor_id: &str,
    notification_type: &str,
    title: &str,
    body: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let mut cursor = channels_col
        .find(doc! { "fixtures": match_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channels: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut all_member_ids = HashSet::new();
    while let Some(channel) = cursor.next().await {
        let channel: Channel = channel.map_err(|e| {
            tracing::error!("❌ Failed to deserialize channel: {}", e);
            AppError::MongoDB(e)
        })?;
        for member in channel.members {
            if member.user_id != actor_id {
                all_member_ids.insert(member.user_id);
            }
        }
    }

    if all_member_ids.is_empty() {
        return Ok(());
    }

    // Broadcast WebSocket
    broadcast_to_match_room(state, match_id, notification_type, payload.clone()).await;

    // Send FCM to offline users
    let fcm_service = match &state.fcm_service {
        Some(s) => s,
        None => return Ok(()),
    };

    let data = serde_json::json!({
        "type": notification_type,
        "match_id": match_id,
        "actor_id": actor_id,
    });

    for user_id in all_member_ids {
        if state.is_user_online(&user_id) {
            let personal_room = format!("user_{}", user_id);
            let tx = state.get_or_create_broadcaster(&personal_room);
            let ws_message = serde_json::json!({
                "type": notification_type,
                "payload": payload,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Ok(json) = serde_json::to_string(&ws_message) {
                let _ = tx.send(json);
            }
        } else {
            let _ = fcm_service
                .send_to_user(
                    state,
                    &user_id,
                    title,
                    body,
                    data.clone(),
                    notification_type,
                )
                .await;
        }
    }

    Ok(())
}

// ============================================================================
// HELPER: Resolve market_id from either ObjectId or business key
// ============================================================================
async fn resolve_market_id(
    markets_col: &Collection<SubFixtureMarket>,
    match_id: &str,
    market_id_or_oid: &str,
) -> Result<String, AppError> {
    if market_id_or_oid.is_empty() {
        return Err(AppError::ValidationError(
            "market_id is required".to_string(),
        ));
    }

    // Try to parse as ObjectId first
    if let Ok(oid) = bson::oid::ObjectId::parse_str(market_id_or_oid) {
        // Look up the market by ObjectId to get the actual market_id
        if let Some(market) = markets_col
            .find_one(doc! { "_id": oid })
            .await
            .map_err(|e| AppError::MongoDB(e))?
        {
            tracing::debug!(
                "✅ Resolved ObjectId {} to market_id: {}",
                market_id_or_oid,
                market.market_id
            );
            return Ok(market.market_id);
        }
    }

    // Not a valid ObjectId or not found, use as-is (business key)
    // But verify the market exists with this market_id
    let market = markets_col
        .find_one(doc! {
            "matchId": match_id,
            "marketId": market_id_or_oid,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| {
            tracing::warn!(
                "❌ Market not found: match={}, market_id={}",
                match_id,
                market_id_or_oid
            );
            AppError::DocumentNotFound
        })?;

    Ok(market.market_id)
}

// ============================================================================
// 1. CREATE SUB-FIXTURE BET
// ============================================================================
pub async fn create_sub_fixture_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateSubFixtureBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("games");
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    tracing::info!(
        "📊 Creating sub-fixture bet: match={}, user={}, selection={}, amount={}",
        payload.match_id,
        payload.starter_id,
        payload.selection,
        payload.amount
    );

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let starter_id = bson::oid::ObjectId::parse_str(&payload.starter_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    // Resolve market_id - accept both ObjectId and business key
    let market_id = resolve_market_id(&markets_col, &payload.match_id, &payload.market_id).await?;

    // Get market for validation
    let market = markets_col
        .find_one(doc! {
            "matchId": &payload.match_id,
            "marketId": &market_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    if market.status != "open" {
        return Err(AppError::ValidationError(format!(
            "Market is not open (status: {})",
            market.status
        )));
    }

    // Validate match exists
    let game = games_col
        .find_one(doc! { "matchId": &payload.match_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    if game.status != "upcoming" && game.status != "not_started" {
        return Err(AppError::ValidationError(
            "Match is no longer accepting bets".to_string(),
        ));
    }

    // Check for duplicate active bet
    let existing = bets_col
        .find_one(doc! {
            "match_id": &payload.match_id,
            "market_id": &market_id,
            "starter_id": starter_id,
            "status": doc! { "$in": ["open", "matched"] },
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if existing.is_some() {
        return Err(AppError::ValidationError(
            "You already have an active bet on this market".to_string(),
        ));
    }

    // Start transaction
    let mut session = state
        .client
        .start_session()
        .await
        .map_err(|e| AppError::MongoDB(e))?;
    session
        .start_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Check balance and deduct
    let user = users_col
        .find_one(doc! { "_id": starter_id })
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    if user.balance < payload.amount {
        session
            .abort_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have {}, need {}",
            user.balance, payload.amount
        )));
    }

    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Create bet with resolved market_id
    let now = BsonDateTime::now();
    let bet = SubFixtureBet {
        id: None,
        match_id: payload.match_id.clone(),
        market_id: market_id.clone(),
        starter_id,
        starter_name: payload.starter_name.clone(),
        starter_selection: payload.selection.clone(),
        starter_amount: payload.amount,
        finisher_id: None,
        finisher_name: None,
        finisher_selection: None,
        finisher_amount: None,
        status: BetStatus::Open,
        total_pot: payload.amount,
        created_at: now,
        updated_at: now,
        settled_at: None,
    };

    let insert_result = bets_col
        .insert_one(&bet)
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let bet_id = insert_result
        .inserted_id
        .as_object_id()
        .map(|oid| oid.to_hex())
        .ok_or_else(|| AppError::InternalServerError("Failed to get bet ID".to_string()))?;

    // Update market pledge counts and totals
    markets_col
        .update_one(
            doc! {
                "matchId": &payload.match_id,
                "marketId": &market_id,
            },
            doc! {
                "$inc": {
                    &format!("pledgeCounts.{}", payload.selection): 1,
                    &format!("pledgeTotals.{}", payload.selection): payload.amount as i32,
                },
                "$set": {
                    "updatedAt": now,
                }
            },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS TO ALL CHANNEL MEMBERS
    // ============================================================
    let pledge_payload = serde_json::json!({
        "market_id": market_id,
        "match_id": payload.match_id,
        "user_id": payload.starter_id,
        "username": payload.starter_name,
        "selection": payload.selection,
        "amount": payload.amount,
        "bet_id": bet_id,
    });

    let _ = notify_sub_fixture_members(
        &state,
        &payload.match_id,
        &payload.starter_id,
        "sub_fixture.pledge",
        "📊 Sub-Fixture Pledge",
        &format!(
            "{} pledged KES {} on {}",
            payload.starter_name, payload.amount, market.market_type
        ),
        pledge_payload,
    )
    .await;

    tracing::info!("✅ Sub-fixture bet created: {}", bet_id);

    Ok(Json(json!({
        "success": true,
        "message": "Sub-fixture bet created successfully",
        "bet_id": bet_id,
        "match_id": payload.match_id,
        "market_id": market_id,
        "status": "open",
        "amount": payload.amount,
        "new_balance": user.balance - payload.amount,
    })))
}

// ============================================================================
// 2. FILL SUB-FIXTURE BET
// ============================================================================
pub async fn fill_sub_fixture_bet_handler(
    State(state): State<AppState>,
    Path(bet_id): Path<String>,
    Json(payload): Json<FillSubFixtureBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("games");
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    tracing::info!(
        "📊 Filling sub-fixture bet: bet_id={}, user={}, selection={}, amount={}",
        bet_id,
        payload.finisher_id,
        payload.selection,
        payload.amount
    );

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let bet_oid = bson::oid::ObjectId::parse_str(&bet_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let finisher_id = bson::oid::ObjectId::parse_str(&payload.finisher_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    // Resolve market_id - accept both ObjectId and business key
    let market_id = resolve_market_id(&markets_col, &payload.match_id, &payload.market_id).await?;

    // Find open bet with resolved market_id
    let bet = bets_col
        .find_one(doc! {
            "_id": bet_oid,
            "status": "open",
            "match_id": &payload.match_id,
            "market_id": &market_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| AppError::ValidationError("Open bet not found".to_string()))?;

    // Validate match
    let game = games_col
        .find_one(doc! { "matchId": &bet.match_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    if game.status != "upcoming" && game.status != "not_started" {
        return Err(AppError::ValidationError(
            "Match is no longer accepting bets".to_string(),
        ));
    }

    // Validate selection is different
    if payload.selection == bet.starter_selection {
        return Err(AppError::ValidationError(
            "Cannot pick the same side as the starter".to_string(),
        ));
    }

    // Validate amount matches
    if (payload.amount - bet.starter_amount).abs() > 0.001 {
        return Err(AppError::ValidationError(format!(
            "Amount must exactly match starter's stake of {}",
            bet.starter_amount
        )));
    }

    // Can't fill your own bet
    if bet.starter_id == finisher_id {
        return Err(AppError::ValidationError(
            "Cannot fill your own bet".to_string(),
        ));
    }

    // Check user hasn't already bet on this market
    let existing = bets_col
        .find_one(doc! {
            "match_id": &bet.match_id,
            "market_id": &bet.market_id,
            "$or": [
                doc! { "starter_id": finisher_id },
                doc! { "finisher_id": finisher_id }
            ],
            "status": doc! { "$in": ["open", "matched"] }
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if existing.is_some() {
        return Err(AppError::ValidationError(
            "You already have an active bet on this market".to_string(),
        ));
    }

    // Start transaction
    let mut session = state
        .client
        .start_session()
        .await
        .map_err(|e| AppError::MongoDB(e))?;
    session
        .start_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Check balance and deduct
    let finisher = users_col
        .find_one(doc! { "_id": finisher_id })
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    if finisher.balance < payload.amount {
        session
            .abort_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have {}, need {}",
            finisher.balance, payload.amount
        )));
    }

    users_col
        .update_one(
            doc! { "_id": finisher_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Update bet
    let now = BsonDateTime::now();
    let update_result = bets_col
        .update_one(
            doc! { "_id": bet_oid, "status": "open" },
            doc! {
                "$set": {
                    "finisher_id": finisher_id,
                    "finisher_name": &payload.finisher_name,
                    "finisher_selection": &payload.selection,
                    "finisher_amount": payload.amount,
                    "status": "matched",
                    "total_pot": bet.starter_amount + payload.amount,
                    "updated_at": now,
                }
            },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if update_result.modified_count == 0 {
        session
            .abort_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        return Err(AppError::ValidationError(
            "Bet was already filled by someone else".to_string(),
        ));
    }

    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS TO ALL CHANNEL MEMBERS
    // ============================================================
    let match_payload = serde_json::json!({
        "bet_id": bet_id,
        "market_id": market_id,
        "match_id": bet.match_id,
        "starter_id": bet.starter_id.to_hex(),
        "starter_name": bet.starter_name,
        "starter_selection": bet.starter_selection,
        "finisher_id": payload.finisher_id,
        "finisher_name": payload.finisher_name,
        "finisher_selection": payload.selection,
        "amount": payload.amount,
        "total_pot": bet.starter_amount + payload.amount,
    });

    // Get market for title
    let market = markets_col
        .find_one(doc! { "matchId": &bet.match_id, "marketId": &market_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let market_type = market
        .as_ref()
        .map(|m| m.market_type.as_str())
        .unwrap_or("sub-fixture");

    let _ = notify_sub_fixture_members(
        &state,
        &bet.match_id,
        &payload.finisher_id,
        "sub_fixture.matched",
        "🤝 Sub-Fixture Matched",
        &format!("{} matched a bet on {}", payload.finisher_name, market_type),
        match_payload,
    )
    .await;

    tracing::info!("✅ Sub-fixture bet filled: {}", bet_id);

    Ok(Json(json!({
        "success": true,
        "message": "Sub-fixture bet filled successfully",
        "bet_id": bet_id,
        "status": "matched",
        "total_pot": bet.starter_amount + payload.amount,
        "new_balance": finisher.balance - payload.amount,
    })))
}

// ============================================================================
// 3. GET OPEN SUB-FIXTURE BETS
// ============================================================================
pub async fn get_open_sub_fixture_bets_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");

    let mut cursor = bets_col
        .find(doc! {
            "match_id": &match_id,
            "status": "open",
        })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        bets.push(SubFixtureBetResponse::from(bet));
    }

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "bets": bets,
        "count": bets.len(),
    })))
}

// ============================================================================
// 4. GET USER'S SUB-FIXTURE BETS
// ============================================================================
pub async fn get_user_sub_fixture_bets_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");

    let user_oid = bson::oid::ObjectId::parse_str(&user_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

    let mut cursor = bets_col
        .find(doc! {
            "$or": [
                { "starter_id": user_oid },
                { "finisher_id": user_oid }
            ]
        })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        bets.push(SubFixtureBetResponse::from(bet));
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "bets": bets,
        "count": bets.len(),
    })))
}

// ============================================================================
// 5. GET SUB-FIXTURE BETS FOR A MARKET (open + matched)
// ============================================================================
pub async fn get_market_sub_fixture_bets_handler(
    State(state): State<AppState>,
    Path((match_id, market_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    // Resolve market_id - accept both ObjectId and business key
    let market_id = resolve_market_id(&markets_col, &match_id, &market_id).await?;

    let mut cursor = bets_col
        .find(doc! {
            "match_id": &match_id,
            "market_id": &market_id,
            "status": doc! { "$in": ["open", "matched"] },
        })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        bets.push(SubFixtureBetResponse::from(bet));
    }

    let total_pot: f64 = bets.iter().map(|b| b.total_pot).sum();

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "market_id": market_id,
        "bets": bets,
        "count": bets.len(),
        "total_pot": total_pot,
    })))
}

// ============================================================================
// 6. SETTLE SUB-FIXTURE BETS FOR A MARKET (internal, reusable logic)
// ============================================================================
pub async fn settle_sub_fixture_bets_for_market(
    state: &Arc<AppState>,
    match_id: &str,
    market_id: &str,
    winning_team: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let bets_collection: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");
    let users_collection: Collection<User> = state.db.collection("users");

    let mut settled_count = 0;
    let mut refund_count = 0;

    // ========================================================================
    // PART 1: SETTLE MATCHED BETS
    // ========================================================================
    let mut session = state
        .client
        .start_session()
        .await
        .map_err(|e| AppError::MongoDB(e))?;
    session
        .start_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let filter = doc! {
        "match_id": match_id,
        "market_id": market_id,
        "status": "matched",
    };

    let mut cursor = bets_collection
        .find(filter)
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    while let Some(bet) = cursor.next(&mut session).await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        let bet_id = bet.id.unwrap();

        match winning_team {
            Some(winner) if winner == bet.starter_selection => {
                users_collection
                    .update_one(
                        doc! { "_id": bet.starter_id },
                        doc! { "$inc": { "balance": bet.total_pot } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                bets_collection
                    .update_one(
                        doc! { "_id": bet_id },
                        doc! {
                            "$set": {
                                "status": "settled",
                                "settled_at": BsonDateTime::now(),
                            }
                        },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                tracing::info!(
                    "✅ Sub-fixture bet {}: Starter won {}",
                    bet_id.to_hex(),
                    bet.total_pot
                );
            }
            Some(winner) if Some(winner) == bet.finisher_selection.as_deref() => {
                if let Some(finisher_id) = bet.finisher_id {
                    users_collection
                        .update_one(
                            doc! { "_id": finisher_id },
                            doc! { "$inc": { "balance": bet.total_pot } },
                        )
                        .session(&mut session)
                        .await
                        .map_err(|e| AppError::MongoDB(e))?;
                }

                bets_collection
                    .update_one(
                        doc! { "_id": bet_id },
                        doc! {
                            "$set": {
                                "status": "settled",
                                "settled_at": BsonDateTime::now(),
                            }
                        },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                tracing::info!(
                    "✅ Sub-fixture bet {}: Finisher won {}",
                    bet_id.to_hex(),
                    bet.total_pot
                );
            }
            _ => {
                // Refund starter
                users_collection
                    .update_one(
                        doc! { "_id": bet.starter_id },
                        doc! { "$inc": { "balance": bet.starter_amount } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                if let Some(finisher_id) = bet.finisher_id {
                    if let Some(finisher_amount) = bet.finisher_amount {
                        users_collection
                            .update_one(
                                doc! { "_id": finisher_id },
                                doc! { "$inc": { "balance": finisher_amount } },
                            )
                            .session(&mut session)
                            .await
                            .map_err(|e| AppError::MongoDB(e))?;
                    }
                }

                bets_collection
                    .update_one(
                        doc! { "_id": bet_id },
                        doc! {
                            "$set": {
                                "status": "refunded",
                                "settled_at": BsonDateTime::now(),
                            }
                        },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                tracing::info!("🔄 Sub-fixture bet {}: Refunded both", bet_id.to_hex());
            }
        }
        settled_count += 1;
    }

    // ========================================================================
    // PART 2: REFUND UNMATCHED BETS
    // ========================================================================
    let open_filter = doc! {
        "match_id": match_id,
        "market_id": market_id,
        "status": "open",
    };

    let mut open_cursor = bets_collection
        .find(open_filter)
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    while let Some(bet) = open_cursor.next(&mut session).await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        let bet_id = bet.id.unwrap();

        users_collection
            .update_one(
                doc! { "_id": bet.starter_id },
                doc! { "$inc": { "balance": bet.starter_amount } },
            )
            .session(&mut session)
            .await
            .map_err(|e| AppError::MongoDB(e))?;

        bets_collection
            .update_one(
                doc! { "_id": bet_id },
                doc! {
                    "$set": {
                        "status": "refunded",
                        "settled_at": BsonDateTime::now(),
                    }
                },
            )
            .session(&mut session)
            .await
            .map_err(|e| AppError::MongoDB(e))?;

        refund_count += 1;
        tracing::info!(
            "🔄 Sub-fixture bet {}: Unmatched - refunded",
            bet_id.to_hex()
        );
    }

    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // ✅ SEND SETTLEMENT NOTIFICATIONS
    // ============================================================
    let settlement_payload = serde_json::json!({
        "market_id": market_id,
        "match_id": match_id,
        "winning_team": winning_team,
        "settled_count": settled_count,
        "refunded_count": refund_count,
    });

    let _ = notify_sub_fixture_members(
        state,
        match_id,
        "system",
        "sub_fixture.settled",
        "⚖️ Sub-Fixture Settled",
        &format!(
            "Market settled. {} bets settled, {} refunded.",
            settled_count, refund_count
        ),
        settlement_payload,
    )
    .await;

    Ok(vec![format!(
        "Settled {} matched bets, refunded {} unmatched bets",
        settled_count, refund_count
    )])
}

// ============================================================================
// 6b. SETTLE SUB-FIXTURE MARKET (HTTP-exposed wrapper)
// ============================================================================
pub async fn settle_sub_fixture_market_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettleSubFixtureMarketRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    tracing::info!(
        "🏁 Settlement request: match={}, market={}, winner={:?}",
        payload.match_id,
        payload.market_id,
        payload.winning_team
    );

    let state_arc = Arc::new(state);
    let messages = settle_sub_fixture_bets_for_market(
        &state_arc,
        &payload.match_id,
        &payload.market_id,
        payload.winning_team.as_deref(),
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "match_id": payload.match_id,
        "market_id": payload.market_id,
        "winning_team": payload.winning_team,
        "messages": messages,
    })))
}

// ============================================================================
// 7. GET MARKETS FOR MATCH
// ============================================================================
pub async fn get_markets_for_match_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    let mut cursor = markets_col
        .find(doc! {
            "matchId": &match_id,
            "isVisible": true,
        })
        .sort(doc! { "createdAt": 1 })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut markets = Vec::new();
    while let Some(market) = cursor.next().await {
        let market: SubFixtureMarket = market.map_err(|e| AppError::MongoDB(e))?;
        markets.push(SubFixtureMarketResponse::from(market));
    }

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "markets": markets,
        "count": markets.len(),
    })))
}

// ============================================================================
// 7b. GET SUB-FIXTURE VISIBILITY FOR MATCH
// ============================================================================
pub async fn get_sub_fixture_visibility_handler(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    let visible_count = markets_col
        .count_documents(doc! {
            "matchId": &match_id,
            "isVisible": true,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "is_visible": visible_count > 0,
    })))
}

// ============================================================================
// 8. GET MARKET DETAILS
// ============================================================================
pub async fn get_market_details_handler(
    State(state): State<AppState>,
    Path((match_id, market_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");

    let market_id = resolve_market_id(&markets_col, &match_id, &market_id).await?;

    let market = markets_col
        .find_one(doc! { "matchId": &match_id, "marketId": &market_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut cursor = bets_col
        .find(doc! { "match_id": &match_id, "market_id": &market_id })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bets = Vec::new();
    let mut open_bets = 0;
    let mut matched_bets = 0;
    let mut settled_bets = 0;
    let mut refunded_bets = 0;
    let mut total_pot = 0.0;

    while let Some(bet) = cursor.next().await {
        let bet: SubFixtureBet = bet.map_err(|e| AppError::MongoDB(e))?;
        match bet.status {
            BetStatus::Open => open_bets += 1,
            BetStatus::Matched => {
                matched_bets += 1;
                total_pot += bet.total_pot;
            }
            BetStatus::Settled => settled_bets += 1,
            BetStatus::Refunded => refunded_bets += 1,
            BetStatus::Cancelled => {}
        }
        bets.push(SubFixtureBetResponse::from(bet));
    }

    Ok(Json(json!({
        "success": true,
        "match_id": match_id,
        "market_id": market_id,
        "market": market.map(SubFixtureMarketResponse::from),
        "stats": {
            "total_bets": bets.len(),
            "open_bets": open_bets,
            "matched_bets": matched_bets,
            "settled_bets": settled_bets,
            "refunded_bets": refunded_bets,
            "total_pot": total_pot,
        },
        "bets": bets,
    })))
}

// ============================================================================
// 10. CLEANUP / AUTO-SETTLE ON MATCH ARCHIVAL
// ============================================================================
// Called from games.rs's move_completed_to_history / cleanup_stale_completed_games
// right before a match is archived. sub_fixture_markets/sub_fixture_bets have
// no foreign-key relationship to Game/HistoryGame -- nothing else in the
// system ever revisits them once the parent match is gone, so any market
// that never got an explicit /sub-fixture/settle call (e.g. the poller
// never classified a clean first_goal/first_card/first_corner event) would
// otherwise sit "open" forever with real money locked in it.
//
// Policy: refund everyone. We deliberately do NOT try to guess a winner
// here -- by the time a match is being archived, whatever signal would
// have let us settle it correctly (a captured first-event) either already
// fired (and the market is no longer "open") or never fired at all, in
// which case there's no reliable winner to declare. Refunding via the
// existing settle_sub_fixture_bets_for_market(..., None) path is the same
// no-winner/draw behavior it already uses elsewhere.
pub async fn cleanup_sub_fixtures_for_match(
    state: &Arc<AppState>,
    match_id: &str,
) -> Result<usize, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    let mut cursor = markets_col
        .find(doc! { "matchId": match_id, "status": "open" })
        .await
        .map_err(AppError::MongoDB)?;

    let mut open_markets = Vec::new();
    while let Some(market) = cursor.next().await {
        open_markets.push(market.map_err(AppError::MongoDB)?);
    }

    if open_markets.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        "🧹 {} unsettled sub-fixture market(s) found for {} at archival -- refunding",
        open_markets.len(),
        match_id
    );

    let mut cleaned = 0;
    for market in open_markets {
        let _ = settle_sub_fixture_bets_for_market(state, match_id, &market.market_id, None)
            .await
            .map_err(|e| {
                tracing::error!(
                    "❌ Failed to refund sub-fixture market {} for {}: {:?}",
                    market.market_id,
                    match_id,
                    e
                );
                e
            });

        markets_col
            .update_one(
                doc! { "matchId": match_id, "marketId": &market.market_id },
                doc! { "$set": { "status": "expired", "updatedAt": BsonDateTime::now() } },
            )
            .await
            .map_err(AppError::MongoDB)?;

        cleaned += 1;
    }

    Ok(cleaned)
}

// ============================================================================
// 11. AUTO-CREATE DEFAULT SUB-FIXTURE MARKETS (called on friendly resolution)
// ============================================================================
// Idempotent -- reuses the same existence check create_sub_fixture_market_handler
// does, so calling this twice for the same match_id is harmless.
pub async fn create_default_markets_for_match(
    state: &AppState,
    match_id: &str,
) -> Result<usize, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    const DEFAULT_MARKET_TYPES: [&str; 3] = ["first_goal", "first_card", "first_corner"];
    let options = vec!["home".to_string(), "away".to_string()];

    let mut created = 0;
    for market_type in DEFAULT_MARKET_TYPES {
        let market_id = format!("{}_{}", match_id, market_type);

        let existing = markets_col
            .find_one(doc! { "matchId": match_id, "marketId": &market_id })
            .await
            .map_err(AppError::MongoDB)?;

        if existing.is_some() {
            continue;
        }

        let now = BsonDateTime::now();
        let mut pledge_counts: HashMap<String, i32> = HashMap::new();
        let mut pledge_totals: HashMap<String, i32> = HashMap::new();
        for opt in &options {
            pledge_counts.insert(opt.clone(), 0);
            pledge_totals.insert(opt.clone(), 0);
        }

        let market = SubFixtureMarket {
            id: None,
            match_id: match_id.to_string(),
            market_id: market_id.clone(),
            market_type: market_type.to_string(),
            options: options.clone(),
            line: None,
            status: "open".to_string(),
            lock_at: None,
            pledge_counts,
            pledge_totals,
            result: None,
            is_visible: true,
            created_at: now,
            updated_at: now,
            settled_at: None,
        };

        markets_col
            .insert_one(&market)
            .await
            .map_err(AppError::MongoDB)?;
        created += 1;
    }

    if created > 0 {
        tracing::info!(
            "📊 Created {} default sub-fixture market(s) for {}",
            created,
            match_id
        );
    }

    Ok(created)
}

// ============================================================================
// 9. CREATE SUB-FIXTURE MARKET
// ============================================================================
pub async fn create_sub_fixture_market_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateSubFixtureMarketRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let markets_col: Collection<SubFixtureMarket> = state.db.collection("sub_fixture_markets");

    if payload.options.is_empty() {
        return Err(AppError::ValidationError(
            "options must not be empty".to_string(),
        ));
    }

    let market_id = format!("{}_{}", payload.match_id, payload.market_type);
    let now = BsonDateTime::now();

    let mut pledge_counts: HashMap<String, i32> = HashMap::new();
    let mut pledge_totals: HashMap<String, i32> = HashMap::new();
    for opt in &payload.options {
        pledge_counts.insert(opt.clone(), 0);
        pledge_totals.insert(opt.clone(), 0);
    }

    tracing::info!(
        "📊 Creating sub-fixture market: match={}, type={}, market_id={}",
        payload.match_id,
        payload.market_type,
        market_id
    );

    let existing = markets_col
        .find_one(doc! { "matchId": &payload.match_id, "marketId": &market_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if let Some(existing_market) = existing {
        return Ok(Json(json!({
            "success": true,
            "created": false,
            "match_id": payload.match_id,
            "market_id": market_id,
            "market": SubFixtureMarketResponse::from(existing_market),
        })));
    }

    let market = SubFixtureMarket {
        id: None,
        match_id: payload.match_id.clone(),
        market_id: market_id.clone(),
        market_type: payload.market_type.clone(),
        options: payload.options.clone(),
        line: payload.line,
        status: "open".to_string(),
        lock_at: payload.lock_at,
        pledge_counts,
        pledge_totals,
        result: None,
        is_visible: true,
        created_at: now,
        updated_at: now,
        settled_at: None,
    };

    markets_col
        .insert_one(&market)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "created": true,
        "match_id": payload.match_id,
        "market_id": market_id,
        "market": SubFixtureMarketResponse::from(market),
    })))
}
