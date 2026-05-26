use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId},
    Collection,
};
use serde::Deserialize;

use crate::{
    errors::{AppError, Result},
    models::bets::{
        Bet, BetResponse, CreateBetRequest, PledgeId, SuccessResponse, UpdateBalanceRequest,
        UpdateBetRequest, UpdatePledgeStatusRequest,
    },
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

// Create a new bet
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

    let collection: Collection<Bet> = state.db.collection("bets");
    let now = Utc::now();

    let bet: Bet = Bet {
        id: Some(ObjectId::new()),
        pledge_id: payload.pledge_id.to_string(),
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
        status: payload.status.clone(),
        winner_id: payload.winner_id,
        winner_username: payload.winner_username,
        winning_selection: payload.winning_selection,
        odds: payload.odds.clone(),
        created_at: now,
        updated_at: now,
        completed_at: None,
    };

    collection.insert_one(&bet).await?;

    println!(
        "✅ Successfully created bet: {} - Total Pot: ₿{}",
        bet.id.as_ref().map(|id| id.to_hex()).unwrap_or_default(),
        payload.total_pot
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

    // user_id is required for this endpoint
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

// Update bet status
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

    let collection: Collection<Bet> = state.db.collection("bets");

    let filter = doc! { "_id": ObjectId::parse_str(&id)? };
    let update = doc! {
        "$set": {
            "winner_id": &payload.winner_id,
            "winner_username": &payload.winner_username,
            "winning_selection": &payload.winning_selection,
            "status": &payload.status,
            "completed_at": Utc::now(),
            "updated_at": Utc::now()
        }
    };

    let options = mongodb::options::FindOneAndUpdateOptions::builder()
        .return_document(mongodb::options::ReturnDocument::After)
        .build();

    let bet = collection
        .find_one_and_update(filter, update)
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let response = BetResponse::from(bet);
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
