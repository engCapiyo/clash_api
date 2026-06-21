use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId, DateTime as BsonDateTime},
    Collection,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    errors::{AppError, Result},
    models::bets::{
        Bet, BetResponse, CreateBetRequest, PledgeId, SuccessResponse, UpdateBalanceRequest,
        UpdateBetRequest, UpdatePledgeStatusRequest,
    },
    models::game::{Bettor, Game, Pledger},
    models::pledges::Pledge,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct BetQuery {
    pub user_id: Option<String>,
    pub status: Option<String>,
    pub pledge_id: Option<String>,
    pub home_team: Option<String>,
    pub away_team: Option<String>,
}

// Get all bets with optional filtering
pub async fn get_bets(
    State(state): State<AppState>,
    Query(query): Query<BetQuery>,
) -> Result<Json<Vec<BetResponse>>> {
    println!("🔍 GET /api/bets called - Starting MongoDB query...");

    let collection: Collection<Bet> = state.db.collection("bets");

    let mut filter = doc! {};

    if let Some(user_id) = &query.user_id {
        filter.insert(
            "$or",
            vec![
                doc! { "starter_id": user_id },
                doc! { "finisher_id": user_id },
            ],
        );
    }

    if let Some(status) = &query.status {
        filter.insert("status", status);
    }

    if let Some(pledge_id) = &query.pledge_id {
        filter.insert("pledge_id", pledge_id);
    }

    if let Some(home_team) = &query.home_team {
        filter.insert("home_team", home_team);
    }

    if let Some(away_team) = &query.away_team {
        filter.insert("away_team", away_team);
    }

    let cursor = collection.find(filter).await?;
    let mut bets: Vec<Bet> = cursor.try_collect().await?;

    bets.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let responses: Vec<BetResponse> = bets.into_iter().map(BetResponse::from).collect();

    println!("✅ Successfully fetched {} bets", responses.len());
    Ok(Json(responses))
}

// ============================================================================
// CREATE BET (ACCEPT PLEDGE) - Moves pledger → bettor
// ============================================================================

pub async fn create_bet(
    State(state): State<AppState>,
    Json(payload): Json<CreateBetRequest>,
) -> Result<Json<BetResponse>> {
    println!(
        "🎯 Creating new bet for pledge: {}",
        payload.pledge_id.to_string()
    );

    // Validate required fields
    if payload.starter_id.is_empty() {
        return Err(AppError::MissingRequiredField("starter_id".to_string()));
    }
    if payload.finisher_id.is_empty() {
        return Err(AppError::MissingRequiredField("finisher_id".to_string()));
    }

    if payload.starter_amount <= 0.0 {
        return Err(AppError::ValidationError(
            "starter_amount must be greater than 0".to_string(),
        ));
    }

    // Validate selections
    let valid_selections = ["home_team", "away_team", "draw"];
    if !valid_selections.contains(&payload.starter_selection.as_str())
        || !valid_selections.contains(&payload.finisher_selection.as_str())
    {
        return Err(AppError::ValidationError(
            "Invalid selection. Must be home_team, away_team or draw".to_string(),
        ));
    }

    if payload.starter_selection == payload.finisher_selection {
        return Err(AppError::ValidationError(
            "Starter and finisher must have opposite selections".to_string(),
        ));
    }

    let finisher_amount = payload
        .finisher_amount
        .unwrap_or_else(|| payload.total_pot - payload.starter_amount);

    if finisher_amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Finisher amount must be greater than 0".to_string(),
        ));
    }

    // ========================================================================
    // 1. GET THE FIXTURE
    // ========================================================================
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let fixture_filter = doc! { "match_id": &payload.match_id };

    let _fixture = games_col
        .find_one(fixture_filter.clone())
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    // ========================================================================
    // 2. FIND AND REMOVE PLEDGER (starter) FROM PLEDGERS ARRAY
    // ========================================================================
    let update_result = games_col
        .update_one(
            doc! {
                "match_id": &payload.match_id,
                "pledgers.userId": &payload.starter_id
            },
            doc! {
                "$pull": {
                    "pledgers": { "userId": &payload.starter_id }
                },
                "$inc": { "pledges": -1 }
            },
        )
        .await?;

    if update_result.matched_count == 0 {
        return Err(AppError::ValidationError(
            "Pledger not found for this fixture".to_string(),
        ));
    }

    // ========================================================================
    // 3. ADD BOTH USERS TO BETTORS ARRAY
    // ========================================================================
    let now = BsonDateTime::from_chrono(Utc::now());
    let bet_id = ObjectId::new();
    let bet_id_str = bet_id.to_hex();

    let starter_bettor = Bettor {
        user_id: payload.starter_id.clone(),
        user_name: payload.starter_username.clone(),
        selection: payload.starter_selection.clone(),
        amount: payload.starter_amount,
        opponent_id: payload.finisher_id.clone(),
        opponent_name: payload.finisher_username.clone(),
        opponent_selection: payload.finisher_selection.clone(),
        opponent_amount: finisher_amount,
        total_pot: payload.total_pot,
        bet_id: bet_id_str.clone(),
        status: Some("active".to_string()),
        winner: None,
        payout: None,
        matched_at: now,
        resolved_at: None,
    };

    let finisher_bettor = Bettor {
        user_id: payload.finisher_id.clone(),
        user_name: payload.finisher_username.clone(),
        selection: payload.finisher_selection.clone(),
        amount: finisher_amount,
        opponent_id: payload.starter_id.clone(),
        opponent_name: payload.starter_username.clone(),
        opponent_selection: payload.starter_selection.clone(),
        opponent_amount: payload.starter_amount,
        total_pot: payload.total_pot,
        bet_id: bet_id_str.clone(),
        status: Some("active".to_string()),
        winner: None,
        payout: None,
        matched_at: now,
        resolved_at: None,
    };

    // Convert to BSON for push
    let starter_bson = bson::to_bson(&starter_bettor)
        .map_err(|e| AppError::InternalServerError(format!("Failed to serialize: {}", e)))?;
    let finisher_bson = bson::to_bson(&finisher_bettor)
        .map_err(|e| AppError::InternalServerError(format!("Failed to serialize: {}", e)))?;

    games_col
        .update_one(
            doc! { "match_id": &payload.match_id },
            doc! {
                "$push": {
                    "bettors": {
                        "$each": [starter_bson, finisher_bson]
                    }
                },
                "$inc": { "bets": 1 }
            },
        )
        .await?;

    // ========================================================================
    // 4. CREATE BET DOCUMENT
    // ========================================================================
    let collection: Collection<Bet> = state.db.collection("bets");
    let now_chrono = Utc::now();

    let bet: Bet = Bet {
        id: Some(bet_id),
        pledge_id: payload.pledge_id.to_string(),
        match_id: payload.match_id.clone(),
        starter_id: payload.starter_id.clone(),
        starter_username: payload.starter_username.clone(),
        starter_selection: payload.starter_selection.clone(),
        starter_amount: payload.starter_amount,
        starter_team: payload.starter_team.clone(),
        finisher_id: payload.finisher_id.clone(),
        finisher_username: payload.finisher_username.clone(),
        finisher_selection: payload.finisher_selection.clone(),
        finisher_amount,
        finisher_team: payload.finisher_team.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        match_time: payload.match_time,
        league: payload.league.clone(),
        sport_type: payload.sport_type.clone(),
        total_pot: payload.total_pot,
        status: "active".to_string(),
        winner_id: None,
        winner_username: None,
        winning_selection: None,
        odds: payload.odds.clone(),
        created_at: now_chrono,
        updated_at: now_chrono,
        completed_at: None,
    };

    collection.insert_one(&bet).await?;

    // ========================================================================
    // 5. UPDATE PLEDGE STATUS TO "matched"
    // ========================================================================
    let pledges_col: Collection<Pledge> = state.db.collection("pledges");
    pledges_col
        .update_one(
            doc! { "_id": ObjectId::parse_str(&payload.pledge_id.to_string())? },
            doc! {
                "$set": {
                    "status": "matched",
                    "bet_id": &bet_id_str,
                    "updated_at": now_chrono
                }
            },
        )
        .await?;

    println!(
        "✅ Successfully created bet: {} - Total Pot: ₿{}",
        bet_id_str, payload.total_pot
    );

    let response = BetResponse::from(bet);
    Ok(Json(response))
}

// Get bet statistics
pub async fn get_bet_stats(
    State(state): State<AppState>,
    Query(query): Query<BetQuery>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting bet statistics...");

    let collection: Collection<Bet> = state.db.collection("bets");

    let mut filter = doc! {};

    if let Some(home_team) = &query.home_team {
        filter.insert("home_team", home_team);
    }

    if let Some(away_team) = &query.away_team {
        filter.insert("away_team", away_team);
    }

    let cursor = collection.find(filter.clone()).await?;
    let bets: Vec<Bet> = cursor.try_collect().await?;

    let total_bets = bets.len() as i64;
    let total_pot: f64 = bets.iter().map(|p| p.total_pot).sum();
    let active_bets = bets.iter().filter(|p| p.status == "active").count() as i64;
    let completed_bets = bets.iter().filter(|p| p.status == "completed").count() as i64;

    let stats = serde_json::json!({
        "total_bets": total_bets,
        "total_pot": total_pot,
        "active_bets": active_bets,
        "completed_bets": completed_bets,
        "average_pot": if total_bets > 0 { total_pot / total_bets as f64 } else { 0.0 }
    });

    println!("✅ Successfully fetched bet statistics");
    Ok(Json(stats))
}

// Get user's betting history
pub async fn get_user_bets(
    State(state): State<AppState>,
    Query(query): Query<BetQuery>,
) -> Result<Json<Vec<BetResponse>>> {
    println!("👤 Getting user bets...");

    let user_id = query
        .user_id
        .ok_or_else(|| AppError::MissingRequiredField("user_id".to_string()))?;

    let collection: Collection<Bet> = state.db.collection("bets");

    let filter = doc! {
        "$or": [
            { "starter_id": &user_id },
            { "finisher_id": &user_id }
        ]
    };

    let cursor = collection.find(filter).await?;
    let mut bets: Vec<Bet> = cursor.try_collect().await?;

    bets.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let responses: Vec<BetResponse> = bets.into_iter().map(BetResponse::from).collect();

    println!("✅ Successfully fetched {} bets for user", responses.len());
    Ok(Json(responses))
}

pub async fn get_recent_bets(State(state): State<AppState>) -> Result<Json<Vec<BetResponse>>> {
    println!("🕒 Getting recent bets...");

    let collection: Collection<Bet> = state.db.collection("bets");

    let cursor = collection.find(doc! {}).await?;
    let mut bets: Vec<Bet> = cursor.try_collect().await?;

    bets.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let recent_bets: Vec<BetResponse> = bets.into_iter().take(10).map(BetResponse::from).collect();

    println!("✅ Successfully fetched {} recent bets", recent_bets.len());
    Ok(Json(recent_bets))
}

// Get a single bet by ID
pub async fn get_bet_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<BetResponse>> {
    println!("🔍 Getting bet by ID: {}", id);

    let collection: Collection<Bet> = state.db.collection("bets");

    let filter = doc! { "_id": ObjectId::parse_str(&id)? };
    let bet = collection
        .find_one(filter)
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let response = BetResponse::from(bet);
    Ok(Json(response))
}

// ============================================================================
// UPDATE BET STATUS (Match Result) - Updates bettors with winner/loser
// ============================================================================

pub async fn update_bet_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateBetRequest>,
) -> Result<Json<BetResponse>> {
    println!("📝 Updating bet {} status to: {}", id, payload.status);

    let valid_selections = ["home_win", "away_win", "draw"];
    if !valid_selections.contains(&payload.winning_selection.as_str()) {
        return Err(AppError::ValidationError(
            "Invalid winning selection".to_string(),
        ));
    }

    // ========================================================================
    // 1. GET THE BET
    // ========================================================================
    let collection: Collection<Bet> = state.db.collection("bets");
    let filter = doc! { "_id": ObjectId::parse_str(&id)? };

    let bet = collection
        .find_one(filter.clone())
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // ========================================================================
    // 2. DETERMINE WINNER (starter or finisher)
    // ========================================================================
    let winner_id = if bet.starter_selection == payload.winning_selection {
        Some(bet.starter_id.clone())
    } else if bet.finisher_selection == payload.winning_selection {
        Some(bet.finisher_id.clone())
    } else {
        None // Draw or no winner
    };

    let winner_username = if let Some(ref wid) = winner_id {
        if wid == &bet.starter_id {
            Some(bet.starter_username.clone())
        } else if wid == &bet.finisher_id {
            Some(bet.finisher_username.clone())
        } else {
            None
        }
    } else {
        None
    };

    // ========================================================================
    // 3. UPDATE BET DOCUMENT
    // ========================================================================
    let update = doc! {
        "$set": {
            "winner_id": &winner_id,
            "winner_username": &winner_username,
            "winning_selection": &payload.winning_selection,
            "status": &payload.status,
            "completed_at": Utc::now(),
            "updated_at": Utc::now()
        }
    };

    let updated_bet = collection
        .find_one_and_update(filter, update)
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // ========================================================================
    // 4. UPDATE FIXXTURE BETTORS WITH WINNER/LOSER
    // ========================================================================
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let now_bson = BsonDateTime::from_chrono(Utc::now());

    // Update each bettor with winner/loser status
    for (user_id, is_winner) in [
        (
            &updated_bet.starter_id,
            Some(&updated_bet.starter_id) == winner_id.as_ref(),
        ),
        (
            &updated_bet.finisher_id,
            Some(&updated_bet.finisher_id) == winner_id.as_ref(),
        ),
    ] {
        let status = if is_winner { "won" } else { "lost" };

        games_col
            .update_one(
                doc! {
                    "match_id": &updated_bet.match_id,
                    "bettors.userId": user_id,
                    "bettors.betId": &id
                },
                doc! {
                    "$set": {
                        "bettors.$.status": status,
                        "bettors.$.winner": is_winner,
                        "bettors.$.payout": if is_winner {
                            updated_bet.total_pot
                        } else {
                            0.0
                        },
                        "bettors.$.resolved_at": now_bson
                    }
                },
            )
            .await?;
    }

    // ========================================================================
    // 5. UPDATE USER BALANCES
    // ========================================================================
    let users_col: Collection<mongodb::bson::Document> = state.db.collection("users");

    if let Some(winner_id) = &winner_id {
        // Winner gets the total pot
        users_col
            .update_one(
                doc! { "id": winner_id },
                doc! {
                    "$inc": { "balance": updated_bet.total_pot }
                },
            )
            .await?;

        // Loser loses their amount
        let loser_id = if &updated_bet.starter_id == winner_id {
            &updated_bet.finisher_id
        } else {
            &updated_bet.starter_id
        };

        users_col
            .update_one(
                doc! { "id": loser_id },
                doc! {
                    "$inc": { "total_bets_lost": 1 }
                },
            )
            .await?;
    }

    println!("✅ Successfully updated bet status");
    let response = BetResponse::from(updated_bet);
    Ok(Json(response))
}

// Update user balance
pub async fn update_user_balance(
    State(state): State<AppState>,
    Json(payload): Json<UpdateBalanceRequest>,
) -> Result<Json<SuccessResponse>> {
    println!("💰 Updating balance for user: {}", payload.user_id);

    if payload.balance < 0.0 {
        return Err(AppError::ValidationError(
            "Balance cannot be negative".to_string(),
        ));
    }

    let collection: Collection<mongodb::bson::Document> = state.db.collection("users");

    let filter = doc! { "id": &payload.user_id };
    let update = doc! {
        "$set": { "balance": payload.balance }
    };

    collection.update_one(filter, update).await?;

    let response = SuccessResponse {
        success: true,
        message: format!("Balance updated to ₿{}", payload.balance),
        data: None,
    };

    Ok(Json(response))
}

// Update pledge status
pub async fn update_pledge_status(
    State(state): State<AppState>,
    Path(pledge_id): Path<String>,
    Json(payload): Json<UpdatePledgeStatusRequest>,
) -> Result<Json<SuccessResponse>> {
    println!(
        "📝 Updating pledge {} status to: {}",
        pledge_id, payload.status
    );

    let valid_statuses = ["matched", "completed", "cancelled"];
    if !valid_statuses.contains(&payload.status.as_str()) {
        return Err(AppError::ValidationError(
            "Invalid pledge status".to_string(),
        ));
    }

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let filter = doc! { "_id": ObjectId::parse_str(&pledge_id)? };
    let update = doc! {
        "$set": {
            "status": &payload.status,
            "updated_at": Utc::now()
        }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    let response = SuccessResponse {
        success: true,
        message: format!("Pledge status updated to '{}'", payload.status),
        data: None,
    };

    Ok(Json(response))
}

// Delete a bet
pub async fn delete_bet(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>> {
    println!("🗑️ Deleting bet: {}", id);

    let collection: Collection<Bet> = state.db.collection("bets");

    let filter = doc! { "_id": ObjectId::parse_str(&id)? };
    let result = collection.delete_one(filter).await?;

    if result.deleted_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    let response = SuccessResponse {
        success: true,
        message: "Bet deleted successfully".to_string(),
        data: None,
    };

    Ok(Json(response))
}
