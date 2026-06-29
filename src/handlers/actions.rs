use crate::{
    errors::AppError,
    models::{
        actions::{Bet, CastVoteRequest, CreateBetRequest, FillBetRequest, SettleBetRequest},
        channel::Channel,
        game::{Game, Voter},
        user::User,
    },
    AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use bson::{doc, to_bson, DateTime as BsonDateTime};
use futures_util::StreamExt;
use mongodb::Collection;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

// ============================================================================
// 1. CAST VOTE (Global - No channel_id)
// ============================================================================
pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let now = BsonDateTime::now();

    // Check if already voted
    let existing = games_col
        .find_one(doc! {
            "match_id": &payload.fixture_id,
            "voters.user_id": &payload.user_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if existing.is_some() {
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    // Create voter
    let voter = Voter {
        user_id: payload.user_id.clone(),
        user_name: payload.username.clone(),
        selection: payload.selection.clone(),
        is_correct: None,
        points_awarded: None,
        voted_at: now,
    };

    let voter_bson = to_bson(&voter)
        .map_err(|e| AppError::ValidationError(format!("BSON serialization error: {}", e)))?;

    // Update fixture with vote
    games_col
        .update_one(
            doc! { "match_id": &payload.fixture_id },
            doc! {
                "$inc": { "votes": 1 },
                "$push": { "voters": voter_bson },
            },
        )
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully",
        "fixture_id": payload.fixture_id,
        "selection": payload.selection,
    })))
}

// ============================================================================
// 2. CREATE BET (Atomic: Bet + Vote) - Starter
// ============================================================================
// File: src/handlers/actions.rs

// ============================================================================
// 2. CREATE BET (Atomic: Bet + Vote) - UPDATED with vote_id
// ============================================================================
// ============================================================================
// 2. CREATE BET (Atomic: Bet + Vote) - FIXED ✅
// ============================================================================
pub async fn create_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let now = BsonDateTime::now();

    // Validate amount
    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    // Parse starter ID
    let starter_id = bson::oid::ObjectId::parse_str(&payload.starter_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let fixture_id = payload.fixture_id.clone();

    // ============================================================
    // 1. VERIFY VOTE EXISTS ✅ FIXED - Check by user_id only
    // ============================================================
    let vote_exists = games_col
        .find_one(doc! {
            "match_id": &fixture_id,
            "voters.user_id": &payload.starter_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if vote_exists.is_none() {
        return Err(AppError::ValidationError(
            "Vote not found. Please vote first before creating a pledge.".to_string(),
        ));
    }

    // ============================================================
    // 2. START TRANSACTION
    // ============================================================
    let mut session = state
        .client
        .start_session()
        .await
        .map_err(|e| AppError::MongoDB(e))?;
    session
        .start_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // 3. FIND USER AND CHECK BALANCE
    // ============================================================
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

    // ============================================================
    // 4. DEDUCT BALANCE
    // ============================================================
    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // 5. CREATE BET DOCUMENT WITH vote_id
    // ============================================================
    let bet = Bet::new_open(
        fixture_id.clone(),
        payload.starter_id.clone(),
        payload.starter_name.clone(),
        payload.starter_selection.clone(),
        payload.amount,
        payload.channel_id.clone(),
        payload.vote_id.clone(), // ✅ Pass vote_id
    );

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

    // ============================================================
    // 6. INCREMENT PLEDGES COUNT IN FIXTURE
    // ============================================================
    games_col
        .update_one(
            doc! { "match_id": &fixture_id },
            doc! { "$inc": { "pledges": 1 } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // ============================================================
    // 7. COMMIT TRANSACTION
    // ============================================================
    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let new_balance = user.balance - payload.amount;

    // ============================================================
    // 8. RETURN SUCCESS
    // ============================================================
    Ok(Json(json!({
        "success": true,
        "message": "Bet created successfully",
        "bet_id": bet_id,
        "vote_id": payload.vote_id,
        "new_balance": new_balance,
        "status": "open",
    })))
}
// ============================================================================
// ✅ NEW: VOTE ROLLBACK ENDPOINT (For pledge failure)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RollbackVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
}

pub async fn rollback_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<RollbackVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    use futures_util::TryStreamExt;
    use mongodb::Collection;

    tracing::info!(
        "🔄 Rolling back vote for user {} on fixture {}",
        payload.user_id,
        payload.fixture_id
    );

    let games_col: Collection<Game> = state.db.collection("fixtures");

    // 1. Find the fixture and get user's vote
    let game = games_col
        .find_one(doc! { "match_id": &payload.fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    // 2. Find the user's vote to know the selection
    let user_vote = game.voters.iter().find(|v| v.user_id == payload.user_id);

    if let Some(vote) = user_vote {
        // 3. Remove from voters array and decrement counts
        let filter = doc! { "match_id": &payload.fixture_id };

        let update = match vote.selection.as_str() {
            "home" => doc! {
                "$pull": { "voters": { "user_id": &payload.user_id } },
                "$inc": { "votes": -1 }
            },
            "away" => doc! {
                "$pull": { "voters": { "user_id": &payload.user_id } },
                "$inc": { "votes": -1 }
            },
            "draw" => doc! {
                "$pull": { "voters": { "user_id": &payload.user_id } },
                "$inc": { "votes": -1 }
            },
            _ => doc! {
                "$pull": { "voters": { "user_id": &payload.user_id } },
                "$inc": { "votes": -1 }
            },
        };

        games_col
            .update_one(filter, update)
            .await
            .map_err(|e| AppError::MongoDB(e))?;

        tracing::info!(
            "✅ Vote rolled back for user {} on fixture {}",
            payload.user_id,
            payload.fixture_id
        );

        Ok(Json(json!({
            "success": true,
            "message": "Vote rolled back successfully",
            "fixture_id": payload.fixture_id,
            "user_id": payload.user_id,
            "selection": vote.selection,
        })))
    } else {
        Err(AppError::ValidationError(
            "User has not voted on this fixture".to_string(),
        ))
    }
}
// ============================================================================
// 3. FILL BET (Atomic: Finisher accepts the bet)
// ============================================================================
// ============================================================================
// 3. FILL BET (Atomic: Finisher accepts the bet)
// ============================================================================
pub async fn fill_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<FillBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let now = BsonDateTime::now();

    // Validate
    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let finisher_id = bson::oid::ObjectId::parse_str(&payload.finisher_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let bet_id = bson::oid::ObjectId::parse_str(&payload.bet_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;

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

    // 1. Find the bet (must be OPEN)
    let bet = bets_col
        .find_one(doc! {
            "_id": bet_id,
            "status": "open",
            "channel_id": &payload.channel_id,
        })
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::ValidationError(
            "Bet not found or already filled".to_string(),
        ))?;

    // 2. Check finisher didn't create this bet
    if bet.starter_id == payload.finisher_id {
        session
            .abort_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        return Err(AppError::ValidationError(
            "Cannot fill your own bet".to_string(),
        ));
    }

    // 3. Find finisher and check balance
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

    // 4. Check finisher didn't already vote
    let existing_vote = games_col
        .find_one(doc! {
            "match_id": &bet.fixture_id,
            "voters.user_id": &payload.finisher_id,
        })
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // 5. Deduct finisher balance
    users_col
        .update_one(
            doc! { "_id": finisher_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // 6. Update bet to MATCHED
    bets_col
        .update_one(
            doc! { "_id": bet_id },
            doc! {
                "$set": {
                    "status": "matched",
                    "finisher_id": &payload.finisher_id,
                    "finisher_name": &payload.finisher_name,
                    "finisher_selection": &payload.finisher_selection,
                    "finisher_amount": payload.amount,
                    "matched_at": now,
                }
            },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // 7. Create vote for finisher (ONLY if not already voted)
    if existing_vote.is_none() {
        let voter = Voter {
            user_id: payload.finisher_id.clone(),
            user_name: payload.finisher_name.clone(),
            selection: payload.finisher_selection.clone(),
            is_correct: None,
            points_awarded: None,
            voted_at: now,
        };

        let voter_bson = to_bson(&voter)
            .map_err(|e| AppError::ValidationError(format!("BSON serialization error: {}", e)))?;

        games_col
            .update_one(
                doc! { "match_id": &bet.fixture_id },
                doc! {
                    "$inc": { "votes": 1 },
                    "$push": { "voters": voter_bson },
                },
            )
            .session(&mut session)
            .await
            .map_err(|e| AppError::MongoDB(e))?;
    }

    // 8. Increment bets count
    games_col
        .update_one(
            doc! { "match_id": &bet.fixture_id },
            doc! { "$inc": { "bets": 1 } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Commit transaction
    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": "Bet filled successfully",
        "bet_id": payload.bet_id,  // ✅ Use payload.bet_id directly (already a valid ObjectId hex)
        "status": "matched",
        "total_pot": bet.starter_amount + payload.amount,
    })))
}

// ============================================================================
// 4. SETTLE BETS (Called by Live Poller when match ends)
// ============================================================================
pub async fn settle_bets_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettleBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let now = BsonDateTime::now();

    // Find all MATCHED bets for this fixture
    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "matched",
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut settled_count = 0;
    let mut failed_count = 0;

    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;

        // Determine winner
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

        // Determine winner_id and results
        let (winner_id, starter_result, finisher_result) = if starter_won && !finisher_won {
            (
                Some(bet.starter_id.clone()),
                Some("won".to_string()),
                Some("lost".to_string()),
            )
        } else if finisher_won && !starter_won {
            (
                bet.finisher_id.clone(),
                Some("lost".to_string()),
                Some("won".to_string()),
            )
        } else {
            (None, Some("draw".to_string()), Some("draw".to_string()))
        };

        // Calculate payout (winner gets total pot)
        let total_pot = bet.starter_amount + bet.finisher_amount.unwrap_or(0.0);

        // Start transaction for each bet
        let mut session = state
            .client
            .start_session()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        session
            .start_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;

        // 1. Update bet status
        let bet_id = bet.id.ok_or(AppError::DocumentNotFound)?;
        bets_col
            .update_one(
                doc! { "_id": bet_id },
                doc! {
                    "$set": {
                        "status": "settled",
                        "winner_id": &winner_id,
                        "starter_result": &starter_result,
                        "finisher_result": &finisher_result,
                        "settled_at": now,
                    }
                },
            )
            .session(&mut session)
            .await
            .map_err(|e| AppError::MongoDB(e))?;

        // 2. Credit winner
        if let Some(winner_id) = &winner_id {
            let winner_oid = bson::oid::ObjectId::parse_str(winner_id)
                .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
            users_col
                .update_one(
                    doc! { "_id": winner_oid },
                    doc! { "$inc": { "balance": total_pot } },
                )
                .session(&mut session)
                .await
                .map_err(|e| AppError::MongoDB(e))?;
        } else {
            // Draw — refund both
            let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id)
                .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
            users_col
                .update_one(
                    doc! { "_id": starter_oid },
                    doc! { "$inc": { "balance": bet.starter_amount } },
                )
                .session(&mut session)
                .await
                .map_err(|e| AppError::MongoDB(e))?;

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
                    .map_err(|e| AppError::MongoDB(e))?;
            }
        }

        // Commit transaction
        session
            .commit_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        settled_count += 1;
    }

    // Update fixture status
    games_col
        .update_one(
            doc! { "match_id": &payload.fixture_id },
            doc! {
                "$set": {
                    "status": "completed",
                    "result": &payload.result,
                    "settled_at": now,
                }
            },
        )
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Settled {} bets", settled_count),
        "settled": settled_count,
        "failed": failed_count,
    })))
}

// ============================================================================
// 5. GET FIXTURE VOTERS (Global - No channel filter)
// ============================================================================
pub async fn get_fixture_voters_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");

    let game = games_col
        .find_one(doc! { "match_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "voters": game.voters,
        "total_votes": game.votes,
    })))
}

// ============================================================================
// 6. GET USER VOTES (Global)
// ============================================================================
pub async fn get_user_votes_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");

    let mut cursor = games_col
        .find(doc! {
            "voters.user_id": &user_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut votes = Vec::new();
    while let Some(game) = cursor.next().await {
        let game: Game = game.map_err(|e| AppError::MongoDB(e))?;
        for voter in game.voters {
            if voter.user_id == user_id {
                votes.push(json!({
                    "fixture_id": game.match_id,
                    "selection": voter.selection,
                    "voted_at": voter.voted_at,
                }));
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "votes": votes,
        "count": votes.len(),
    })))
}

// ============================================================================
// 7. CHECK USER VOTE (Global)
// ============================================================================
pub async fn check_user_vote_handler(
    State(state): State<AppState>,
    Path((fixture_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");

    let game = games_col
        .find_one(doc! { "match_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let user_vote = game.voters.iter().find(|v| v.user_id == user_id);

    Ok(Json(json!({
        "success": true,
        "has_voted": user_vote.is_some(),
        "selection": user_vote.map(|v| &v.selection),
    })))
}

// ============================================================================
// 8. GET CHANNEL BETTORS (Channel-Specific)
// ============================================================================
pub async fn get_channel_bettors_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");

    let mut cursor = bets_col
        .find(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
            "status": { "$ne": "open" }, // Only show matched + settled bets
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bettors = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        bettors.push(bet);
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "bettors": bettors,
        "count": bettors.len(),
    })))
}

// ============================================================================
// 9. GET OPEN BETS (Channel-Specific)
// ============================================================================
pub async fn get_open_bets_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");

    let mut cursor = bets_col
        .find(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
            "status": "open",
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut open_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        open_bets.push(bet);
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "open_bets": open_bets,
        "count": open_bets.len(),
    })))
}

// ============================================================================
// 10. GET USER'S BETS (Global - Across all channels)
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
        .map_err(|e| AppError::MongoDB(e))?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        bets.push(bet);
    }

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "bets": bets,
        "count": bets.len(),
    })))
}

// ============================================================================
// 11. GET CHANNEL MEMBERS (Helper for filtering voters)
// ============================================================================
pub async fn get_channel_members_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
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
// 12. GET CHANNEL VOTERS (Filtered by channel membership)
// ============================================================================
pub async fn get_channel_voters_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    // 1. Get all voters (global)
    let game = games_col
        .find_one(doc! { "match_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    // 2. Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // 3. Filter voters by channel membership
    let channel_voters: Vec<Voter> = game
        .voters
        .into_iter()
        .filter(|v| member_ids.contains(&v.user_id))
        .collect();

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "voters": channel_voters,
        "total": channel_voters.len(),
    })))
}

// ============================================================================
// 13. GET CHANNEL PLEDGES (Filtered by channel membership)
// ============================================================================
pub async fn get_channel_pledges_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let bets_col: Collection<Bet> = state.db.collection("bets");

    // 1. Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // 2. Get open bets for this channel + fixture
    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": "open",
            "channel_id": &channel_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut open_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        // Check if starter is in channel
        if member_ids.contains(&bet.starter_id) {
            open_bets.push(bet);
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
