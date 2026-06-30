use crate::{
    errors::AppError,
    models::{
        actions::{
            Bet, CastVoteRequest, CreateBetRequest, FillBetRequest, RollbackVoteRequest,
            SettleBetRequest, Vote,
        },
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
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let games_col: Collection<Game> = state.db.collection("fixtures");

    tracing::info!(
        "🗳️ Cast vote: fixture={}, user={}, selection={}",
        payload.fixture_id,
        payload.user_id,
        payload.selection
    );

    // Check if already voted
    let existing = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    if existing.is_some() {
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    // Insert vote
    let vote = Vote::new(
        payload.fixture_id.clone(),
        payload.user_id.clone(),
        payload.username.clone(),
        payload.selection.clone(),
    );

    votes_col
        .insert_one(&vote)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Increment vote count
    games_col
        .update_one(
            doc! { "match_id": &payload.fixture_id },
            doc! { "$inc": { "votes": 1 } },
        )
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully",
        "fixture_id": payload.fixture_id,
        "selection": payload.selection,
        "vote_id": payload.user_id,
    })))
}

// ============================================================================
// 2. CREATE BET (Atomic: Bet + Vote)
// ============================================================================
pub async fn create_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let starter_id = bson::oid::ObjectId::parse_str(&payload.starter_id)
        .map_err(|e| AppError::InvalidObjectId(e.to_string()))?;
    let fixture_id = payload.fixture_id.clone();

    // Check if already voted
    let vote_exists = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &payload.starter_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

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

    // Check balance
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

    // Auto-cast vote if not already voted
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
            .map_err(|e| AppError::MongoDB(e))?;

        games_col
            .update_one(
                doc! { "match_id": &fixture_id },
                doc! { "$inc": { "votes": 1 } },
            )
            .session(&mut session)
            .await
            .map_err(|e| AppError::MongoDB(e))?;
    }

    // Deduct balance
    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    // Create bet
    let bet = Bet::new_open(
        fixture_id.clone(),
        payload.starter_id.clone(),
        payload.starter_name.clone(),
        payload.starter_selection.clone(),
        payload.amount,
        payload.channel_id.clone().unwrap_or_default(),
        payload.vote_id.clone(),
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

    // Increment pledges count
    games_col
        .update_one(
            doc! { "match_id": &fixture_id },
            doc! { "$inc": { "pledges": 1 } },
        )
        .session(&mut session)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    session
        .commit_transaction()
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let new_balance = user.balance - payload.amount;

    Ok(Json(json!({
        "success": true,
        "message": "Pledge created successfully",
        "bet_id": bet_id,
        "vote_id": payload.vote_id,
        "new_balance": new_balance,
        "status": "open",
        "auto_voted": vote_exists.is_none(),
    })))
}

// ============================================================================
// 3. ROLLBACK VOTE
// ============================================================================
pub async fn rollback_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<RollbackVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let games_col: Collection<Game> = state.db.collection("fixtures");

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::ValidationError(
            "User has not voted on this fixture".to_string(),
        ))?;

    votes_col
        .delete_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    games_col
        .update_one(
            doc! { "match_id": &payload.fixture_id },
            doc! { "$inc": { "votes": -1 } },
        )
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": "Vote rolled back successfully",
        "fixture_id": payload.fixture_id,
        "user_id": payload.user_id,
        "selection": vote.selection,
    })))
}

// ============================================================================
// 4. FILL BET - NO channel_id in query
// ============================================================================
// ============================================================================
// 4. FILL BET (Atomic: Finisher accepts the bet)
// ============================================================================
pub async fn fill_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<FillBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
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

    // ============================================================
    // 1. Find the bet (must be OPEN) - NO channel filter
    // ============================================================
    let bet = bets_col
        .find_one(doc! {
            "_id": bet_id,
            "status": "open",
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
    let existing_vote = votes_col
        .find_one(doc! {
            "fixture_id": &bet.fixture_id,
            "user_id": &payload.finisher_id,
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

    // 7. Create vote for finisher if not already voted
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
            .map_err(|e| AppError::MongoDB(e))?;

        // Increment vote count in fixture
        games_col
            .update_one(
                doc! { "match_id": &bet.fixture_id },
                doc! { "$inc": { "votes": 1 } },
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

    tracing::info!(
        "✅ Bet filled: bet_id={}, finisher={}, fixture={}",
        payload.bet_id,
        payload.finisher_id,
        bet.fixture_id
    );

    Ok(Json(json!({
        "success": true,
        "message": "Bet filled successfully",
        "bet_id": payload.bet_id,
        "status": "matched",
        "total_pot": bet.starter_amount + payload.amount,
    })))
}

// ============================================================================
// 5. SETTLE BETS
// ============================================================================
pub async fn settle_bets_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettleBetRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let users_col: Collection<User> = state.db.collection("users");
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let now = BsonDateTime::now();

    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "matched",
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut settled_count = 0;

    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;

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

        let total_pot = bet.starter_amount + bet.finisher_amount.unwrap_or(0.0);

        let mut session = state
            .client
            .start_session()
            .await
            .map_err(|e| AppError::MongoDB(e))?;
        session
            .start_transaction()
            .await
            .map_err(|e| AppError::MongoDB(e))?;

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

    // Update votes with correctness
    let update_result = votes_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! {
                "$set": {
                    "is_correct": true,
                    "points_awarded": 1,
                }
            },
        )
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Settled {} bets", settled_count),
        "settled": settled_count,
        "votes_updated": update_result.modified_count,
    })))
}

// ============================================================================
// 6. GET FIXTURE VOTERS (Global - No filter)
// ============================================================================
pub async fn get_fixture_voters_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut voters = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| AppError::MongoDB(e))?;
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
// 7. GET USER VOTES (Global)
// ============================================================================
pub async fn get_user_votes_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let mut cursor = votes_col
        .find(doc! { "user_id": &user_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut votes = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| AppError::MongoDB(e))?;
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
// 8. CHECK USER VOTE (Global)
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
        .map_err(|e| AppError::MongoDB(e))?;

    Ok(Json(json!({
        "success": true,
        "has_voted": vote.is_some(),
        "selection": vote.as_ref().map(|v| v.selection.clone()),
        "is_correct": vote.as_ref().and_then(|v| v.is_correct),
        "points_awarded": vote.as_ref().and_then(|v| v.points_awarded),
    })))
}

// ============================================================================
// 9. GET CHANNEL VOTERS (Filtered by channel membership)
// ============================================================================
pub async fn get_channel_voters_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    // Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // Get ALL votes for fixture (global)
    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut voters = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| AppError::MongoDB(e))?;
        // Filter: only show voters who are in the channel
        if member_ids.contains(&vote.user_id) {
            voters.push(json!({
                "user_id": vote.user_id,
                "user_name": vote.user_name,
                "selection": vote.selection,
                "voted_at": vote.voted_at,
                "is_correct": vote.is_correct,
                "points_awarded": vote.points_awarded,
            }));
        }
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "voters": voters,
        "total": voters.len(),
    })))
}

// ============================================================================
// 10. GET CHANNEL PLEDGES (Filtered by channel membership)
// ============================================================================
pub async fn get_channel_pledges_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    // Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // Get ALL open bets for fixture (global)
    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": "open",
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut open_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        // Filter: only show bets where starter is in the channel
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

// ============================================================================
// 11. GET CHANNEL BETTORS (Filtered by channel membership - BOTH parties)
// ============================================================================
pub async fn get_channel_bettors_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    // Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // Get ALL non-open bets for fixture (global)
    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": { "$ne": "open" },
        })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut matched_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| AppError::MongoDB(e))?;
        // Filter: BOTH starter AND finisher must be in the channel
        if let Some(finisher_id) = &bet.finisher_id {
            if member_ids.contains(&bet.starter_id) && member_ids.contains(finisher_id) {
                matched_bets.push(bet);
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
// 12. GET CHANNEL MEMBERS (Helper)
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
// 13. GET USER'S BETS (Global - Across all channels)
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
// 14. GET VOTE COUNT (Fast count from fixture cache)
// ============================================================================
pub async fn get_vote_count_handler(
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
        "vote_count": game.votes,
    })))
}

// ============================================================================
// 15. GET VOTE BREAKDOWN
// ============================================================================
pub async fn get_vote_breakdown_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let pipeline = vec![
        doc! { "$match": { "fixture_id": &fixture_id } },
        doc! { "$group": {
            "_id": "$selection",
            "count": { "$sum": 1 }
        }},
    ];

    let mut cursor = votes_col
        .aggregate(pipeline)
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut breakdown = serde_json::Map::new();
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(|e| AppError::MongoDB(e))?;
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
        "breakdown": {
            "home": home,
            "away": away,
            "draw": draw,
        },
        "total": home + away + draw,
    })))
}
