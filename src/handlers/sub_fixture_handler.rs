use crate::{
    errors::AppError,
    models::{
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
use std::collections::HashMap;
use std::sync::Arc;

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
// 2. FILL SUB-FIXTURE BET - FIXED
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
// 5. GET MATCHED SUB-FIXTURE BETS FOR A MARKET
// ============================================================================
pub async fn get_market_sub_fixture_bets_handler(
    State(state): State<AppState>,
    Path((match_id, market_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<SubFixtureBet> = state.db.collection("sub_fixture_bets");

    let mut cursor = bets_col
        .find(doc! {
            "match_id": &match_id,
            "market_id": &market_id,
            "status": "matched",
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

    // Find all MATCHED bets for this market
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
                // ✅ CASE 1: STARTER WINS - gets full pot
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
                    "✅ Sub-fixture bet {}: Starter {} won {} (total pot)",
                    bet_id.to_hex(),
                    bet.starter_id.to_hex(),
                    bet.total_pot
                );
            }
            Some(winner) if Some(winner) == bet.finisher_selection.as_deref() => {
                // ✅ CASE 2: FINISHER WINS - gets full pot
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
                    "✅ Sub-fixture bet {}: Finisher {} won {} (total pot)",
                    bet_id.to_hex(),
                    bet.finisher_id
                        .map(|id| id.to_hex())
                        .unwrap_or("unknown".to_string()),
                    bet.total_pot
                );
            }
            _ => {
                // ✅ CASE 3: DRAW / NO WINNER - REFUND BOTH
                // Refund starter
                users_collection
                    .update_one(
                        doc! { "_id": bet.starter_id },
                        doc! { "$inc": { "balance": bet.starter_amount } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| AppError::MongoDB(e))?;

                // Refund finisher
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

                tracing::info!(
                    "🔄 Sub-fixture bet {}: Draw/No winner - refunded both parties",
                    bet_id.to_hex()
                );
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

        // ✅ REFUND STARTER - no one filled their bet
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
            "🔄 Sub-fixture bet {}: Unmatched - refunded starter {} ({} KES)",
            bet_id.to_hex(),
            bet.starter_id.to_hex(),
            bet.starter_amount
        );
    }

    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

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
