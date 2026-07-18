use crate::{
    errors::AppError,
    models::{
        actions::{
            Bet, CastVoteRequest, CreateBetRequest, FillBetRequest, RollbackVoteRequest,
            SettleBetRequest, Vote,
        },
        channel::{Channel, ChannelFixture, VoteCounts},
        game::Game,
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
// HELPER: Get all channel IDs where user is a member
// ============================================================================
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

    // Build the filter
    let filter = doc! {
        "channel_id": channel_id,
        "fixture_id": fixture_id,
    };

    // Build the update document
    let mut update = doc! {};

    // Add $setOnInsert for new documents
    let mut set_on_insert = doc! {
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "status": set_on_insert_status,
        "comment_count": 0,
        "pledge_count": 0,
        "bet_count": 0,
        "likes_count": 0,
        "unread_counts": doc! {},
        "added_at": BsonDateTime::now(),
    };

    // Add vote_counts as a nested document
    let vote_counts_doc = doc! {
        "home": 0,
        "away": 0,
        "draw": 0,
    };
    set_on_insert.insert("vote_counts", vote_counts_doc);

    update.insert("$setOnInsert", set_on_insert);

    // Add $inc if there's an increment field
    if let Some(field) = increment_field {
        let mut inc = doc! {};
        inc.insert(field, increment_value);
        update.insert("$inc", inc);
    }

    tracing::debug!("📤 Update document: {:?}", update);

    // Execute the update with upsert
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
// 1. CAST VOTE — Creates/Updates channel_fixtures for ALL user's channels
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

    // Check if already voted globally
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

    // Insert vote (global)
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

    tracing::debug!("✅ Vote inserted successfully");

    let increment_field = match payload.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => {
            tracing::error!("❌ Invalid selection: {}", payload.selection);
            return Err(AppError::ValidationError(
                "Invalid selection. Must be 'home', 'away', or 'draw'".to_string(),
            ));
        }
    };

    // Get ALL channels the user belongs to
    let channel_ids = get_user_channel_ids(&channels_col, &payload.user_id).await?;

    tracing::info!(
        "📊 User is in {} channels, updating vote counts for fixture {}",
        channel_ids.len(),
        payload.fixture_id
    );

    // Create/Update ChannelFixture for EACH channel
    for channel_id in &channel_ids {
        tracing::debug!("🔄 Processing channel: {}", channel_id);
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
// 2. CREATE BET — Creates/Updates channel_fixtures for ALL user's channels
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

    tracing::info!(
        "💰 Create bet: fixture={}, user={}, amount={}",
        payload.fixture_id,
        payload.starter_id,
        payload.amount
    );

    if payload.amount <= 0.0 {
        tracing::warn!("⚠️ Invalid amount: {}", payload.amount);
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let starter_id = bson::oid::ObjectId::parse_str(&payload.starter_id).map_err(|e| {
        tracing::error!("❌ Invalid starter_id: {}", e);
        AppError::InvalidObjectId(e.to_string())
    })?;
    let fixture_id = payload.fixture_id.clone();

    // Check if already voted
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

    // Start transaction
    let mut session = state.client.start_session().await.map_err(|e| {
        tracing::error!("❌ Failed to start session: {}", e);
        AppError::MongoDB(e)
    })?;
    session.start_transaction().await.map_err(|e| {
        tracing::error!("❌ Failed to start transaction: {}", e);
        AppError::MongoDB(e)
    })?;

    tracing::debug!("✅ Transaction started");

    // Check balance
    let user = users_col
        .find_one(doc! { "_id": starter_id })
        .session(&mut session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find user: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::error!("❌ User not found: {}", payload.starter_id);
            AppError::DocumentNotFound
        })?;

    tracing::debug!("💰 User balance: {}", user.balance);

    if user.balance < payload.amount {
        tracing::warn!(
            "⚠️ Insufficient balance: {} < {}",
            user.balance,
            payload.amount
        );
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
        tracing::debug!("🔄 Auto-casting vote for user");
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
            .map_err(|e| {
                tracing::error!("❌ Failed to insert auto-vote: {}", e);
                AppError::MongoDB(e)
            })?;

        let increment_field = match payload.starter_selection.as_str() {
            "home" => "vote_counts.home",
            "away" => "vote_counts.away",
            "draw" => "vote_counts.draw",
            _ => {
                tracing::error!("❌ Invalid selection: {}", payload.starter_selection);
                session.abort_transaction().await?;
                return Err(AppError::ValidationError("Invalid selection".to_string()));
            }
        };

        // Get user's channels
        let channel_ids = get_user_channel_ids(&channels_col, &payload.starter_id).await?;

        // Update channel_fixtures for EACH channel
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

    // Deduct balance
    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to deduct balance: {}", e);
            AppError::MongoDB(e)
        })?;

    tracing::debug!("💰 Balance deducted");

    // Create bet
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
        .map_err(|e| {
            tracing::error!("❌ Failed to create bet: {}", e);
            AppError::MongoDB(e)
        })?;

    let bet_id = insert_result
        .inserted_id
        .as_object_id()
        .map(|oid| oid.to_hex())
        .ok_or_else(|| {
            tracing::error!("❌ Failed to get bet ID");
            AppError::InternalServerError("Failed to get bet ID".to_string())
        })?;

    tracing::debug!("✅ Bet created: {}", bet_id);

    // Get user's channels for pledge count update
    let channel_ids = get_user_channel_ids(&channels_col, &payload.starter_id).await?;

    // Update pledge_count for EACH channel
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

    session.commit_transaction().await.map_err(|e| {
        tracing::error!("❌ Failed to commit transaction: {}", e);
        AppError::MongoDB(e)
    })?;

    tracing::info!(
        "✅ Bet created successfully: {} for user {}",
        bet_id,
        payload.starter_id
    );

    let new_balance = user.balance - payload.amount;

    Ok(Json(json!({
        "success": true,
        "message": "Pledge created successfully",
        "bet_id": bet_id,
        "vote_id": payload.vote_id,
        "new_balance": new_balance,
        "status": "open",
        "auto_voted": vote_exists.is_none(),
        "channels_updated": channel_ids.len(),
    })))
}

// ============================================================================
// 3. ROLLBACK VOTE — Decrements channel_fixtures for ALL user's channels
// ============================================================================
pub async fn rollback_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<RollbackVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

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
        .map_err(|e| {
            tracing::error!("❌ Failed to find vote: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or(AppError::ValidationError(
            "User has not voted on this fixture".to_string(),
        ))?;

    tracing::debug!("✅ Found vote: selection={}", vote.selection);

    votes_col
        .delete_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to delete vote: {}", e);
            AppError::MongoDB(e)
        })?;

    tracing::debug!("✅ Vote deleted");

    let decrement_field = match vote.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => {
            tracing::error!("❌ Invalid selection in vote record: {}", vote.selection);
            return Err(AppError::ValidationError(
                "Invalid selection in vote record".to_string(),
            ));
        }
    };

    // Get ALL channels the user belongs to
    let channel_ids = get_user_channel_ids(&channels_col, &payload.user_id).await?;

    tracing::info!(
        "📊 User is in {} channels, decrementing vote counts",
        channel_ids.len()
    );

    // Decrement channel_fixtures for EACH channel
    for channel_id in &channel_ids {
        tracing::debug!("🔄 Decrementing channel: {}", channel_id);
        channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": channel_id,
                    "fixture_id": &payload.fixture_id,
                },
                doc! { "$inc": { decrement_field: -1 } },
            )
            .await
            .map_err(|e| {
                tracing::error!("❌ Failed to decrement channel_fixture: {}", e);
                AppError::MongoDB(e)
            })?;
    }

    tracing::info!(
        "✅ Vote rolled back for user {} in {} channels",
        payload.user_id,
        channel_ids.len()
    );

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
// 4. FILL BET — Creates/Updates channel_fixtures for BOTH user's channels
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

    // Validate
    if payload.amount <= 0.0 {
        tracing::warn!("⚠️ Invalid amount: {}", payload.amount);
        return Err(AppError::ValidationError(
            "Amount must be greater than 0".to_string(),
        ));
    }

    let finisher_id = bson::oid::ObjectId::parse_str(&payload.finisher_id).map_err(|e| {
        tracing::error!("❌ Invalid finisher_id: {}", e);
        AppError::InvalidObjectId(e.to_string())
    })?;
    let bet_id = bson::oid::ObjectId::parse_str(&payload.bet_id).map_err(|e| {
        tracing::error!("❌ Invalid bet_id: {}", e);
        AppError::InvalidObjectId(e.to_string())
    })?;

    // Start transaction
    let mut session = state.client.start_session().await.map_err(|e| {
        tracing::error!("❌ Failed to start session: {}", e);
        AppError::MongoDB(e)
    })?;
    session.start_transaction().await.map_err(|e| {
        tracing::error!("❌ Failed to start transaction: {}", e);
        AppError::MongoDB(e)
    })?;

    tracing::debug!("✅ Transaction started");

    // 1. Find the bet (must be OPEN)
    let bet = bets_col
        .find_one(doc! {
            "_id": bet_id,
            "status": "open",
        })
        .session(&mut session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find bet: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or(AppError::ValidationError(
            "Bet not found or already filled".to_string(),
        ))?;

    tracing::debug!("✅ Found bet: {}", payload.bet_id);

    // 2. Check finisher didn't create this bet
    if bet.starter_id == payload.finisher_id {
        tracing::warn!("⚠️ Cannot fill own bet");
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
        .map_err(|e| {
            tracing::error!("❌ Failed to find finisher: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::error!("❌ Finisher not found: {}", payload.finisher_id);
            AppError::DocumentNotFound
        })?;

    tracing::debug!("💰 Finisher balance: {}", finisher.balance);

    if finisher.balance < payload.amount {
        tracing::warn!(
            "⚠️ Insufficient balance: {} < {}",
            finisher.balance,
            payload.amount
        );
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
        .map_err(|e| {
            tracing::error!("❌ Failed to check finisher vote: {}", e);
            AppError::MongoDB(e)
        })?;

    // 5. Deduct finisher balance
    users_col
        .update_one(
            doc! { "_id": finisher_id },
            doc! { "$inc": { "balance": -payload.amount } },
        )
        .session(&mut session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to deduct finisher balance: {}", e);
            AppError::MongoDB(e)
        })?;

    tracing::debug!("💰 Finisher balance deducted");

    // 6. Update bet to MATCHED
    let finisher_id_clone = payload.finisher_id.clone();
    let finisher_name_clone = payload.finisher_name.clone();
    let finisher_selection_clone = payload.finisher_selection.clone();

    bets_col
        .update_one(
            doc! { "_id": bet_id },
            doc! {
                "$set": {
                    "status": "matched",
                    "finisher_id": finisher_id_clone,
                    "finisher_name": finisher_name_clone,
                    "finisher_selection": finisher_selection_clone,
                    "finisher_amount": payload.amount,
                    "matched_at": now,
                }
            },
        )
        .session(&mut session)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to update bet: {}", e);
            AppError::MongoDB(e)
        })?;

    tracing::debug!("✅ Bet updated to matched");

    // 7. Create vote for finisher if not already voted
    if existing_vote.is_none() {
        tracing::debug!("🔄 Auto-casting vote for finisher");
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
            .map_err(|e| {
                tracing::error!("❌ Failed to insert finisher vote: {}", e);
                AppError::MongoDB(e)
            })?;

        let increment_field = match payload.finisher_selection.as_str() {
            "home" => "vote_counts.home",
            "away" => "vote_counts.away",
            "draw" => "vote_counts.draw",
            _ => {
                tracing::error!(
                    "❌ Invalid finisher selection: {}",
                    payload.finisher_selection
                );
                session.abort_transaction().await?;
                return Err(AppError::ValidationError("Invalid selection".to_string()));
            }
        };

        // Get finisher's channels
        let finisher_channel_ids =
            get_user_channel_ids(&channels_col, &payload.finisher_id).await?;

        // Update channel_fixtures for EACH of finisher's channels
        for channel_id in &finisher_channel_ids {
            upsert_channel_fixture(
                &channel_fixtures_col,
                channel_id,
                &bet.fixture_id,
                Some(increment_field),
                1,
                "active",
            )
            .await?;
        }
    }

    // Get BOTH starter and finisher channel IDs
    let starter_channel_ids = get_user_channel_ids(&channels_col, &bet.starter_id).await?;
    let finisher_channel_ids = get_user_channel_ids(&channels_col, &payload.finisher_id).await?;
    let all_channel_ids: HashSet<String> = starter_channel_ids
        .into_iter()
        .chain(finisher_channel_ids.into_iter())
        .collect();

    tracing::info!(
        "📊 Updating bet_count for {} channels",
        all_channel_ids.len()
    );

    // Update bet_count for ALL channels where either user is a member
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

    // Commit transaction
    session.commit_transaction().await.map_err(|e| {
        tracing::error!("❌ Failed to commit transaction: {}", e);
        AppError::MongoDB(e)
    })?;

    tracing::info!(
        "✅ Bet filled: bet_id={}, finisher={}, fixture={}, channels_updated={}",
        payload.bet_id,
        payload.finisher_id,
        bet.fixture_id,
        all_channel_ids.len()
    );

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
// 5. SETTLE BETS — Updates channel_fixtures status only
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

    // ========================================================================
    // PART 1: SETTLE MATCHED BETS
    // ========================================================================
    let mut matched_cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "matched",
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find matched bets: {}", e);
            AppError::MongoDB(e)
        })?;

    while let Some(bet) = matched_cursor.next().await {
        let bet: Bet = bet.map_err(|e| {
            tracing::error!("❌ Failed to deserialize bet: {}", e);
            AppError::MongoDB(e)
        })?;

        tracing::debug!("🔄 Processing bet: {:?}", bet.id);

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

        let mut session = state.client.start_session().await.map_err(|e| {
            tracing::error!("❌ Failed to start session: {}", e);
            AppError::MongoDB(e)
        })?;
        session.start_transaction().await.map_err(|e| {
            tracing::error!("❌ Failed to start transaction: {}", e);
            AppError::MongoDB(e)
        })?;

        let bet_id = bet.id.ok_or(AppError::DocumentNotFound)?;

        // CASE 1: STARTER WINS
        if starter_won && !finisher_won {
            tracing::debug!("🏆 Starter wins");
            let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id).map_err(|e| {
                tracing::error!("❌ Invalid starter_id: {}", e);
                AppError::InvalidObjectId(e.to_string())
            })?;

            users_col
                .update_one(
                    doc! { "_id": starter_oid },
                    doc! { "$inc": { "balance": total_pot } },
                )
                .session(&mut session)
                .await
                .map_err(|e| {
                    tracing::error!("❌ Failed to update starter balance: {}", e);
                    AppError::MongoDB(e)
                })?;

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! {
                        "$set": {
                            "status": "settled",
                            "winner_id": &bet.starter_id,
                            "starter_result": "won",
                            "finisher_result": "lost",
                            "settled_at": now,
                        }
                    },
                )
                .session(&mut session)
                .await
                .map_err(|e| {
                    tracing::error!("❌ Failed to update bet: {}", e);
                    AppError::MongoDB(e)
                })?;

            tracing::info!(
                "✅ Bet {}: Starter {} won {} (total pot)",
                bet_id.to_hex(),
                bet.starter_id,
                total_pot
            );
        }
        // CASE 2: FINISHER WINS
        else if finisher_won && !starter_won {
            tracing::debug!("🏆 Finisher wins");
            if let Some(finisher_id) = &bet.finisher_id {
                let finisher_oid = bson::oid::ObjectId::parse_str(finisher_id).map_err(|e| {
                    tracing::error!("❌ Invalid finisher_id: {}", e);
                    AppError::InvalidObjectId(e.to_string())
                })?;

                users_col
                    .update_one(
                        doc! { "_id": finisher_oid },
                        doc! { "$inc": { "balance": total_pot } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| {
                        tracing::error!("❌ Failed to update finisher balance: {}", e);
                        AppError::MongoDB(e)
                    })?;
            }

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! {
                        "$set": {
                            "status": "settled",
                            "winner_id": &bet.finisher_id,
                            "starter_result": "lost",
                            "finisher_result": "won",
                            "settled_at": now,
                        }
                    },
                )
                .session(&mut session)
                .await
                .map_err(|e| {
                    tracing::error!("❌ Failed to update bet: {}", e);
                    AppError::MongoDB(e)
                })?;

            tracing::info!(
                "✅ Bet {}: Finisher {} won {} (total pot)",
                bet_id.to_hex(),
                bet.finisher_id.as_deref().unwrap_or("unknown"),
                total_pot
            );
        }
        // CASE 3: DRAW / NO WINNER - REFUND BOTH
        else {
            tracing::debug!("🔄 Draw - refunding both");
            let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id).map_err(|e| {
                tracing::error!("❌ Invalid starter_id: {}", e);
                AppError::InvalidObjectId(e.to_string())
            })?;
            users_col
                .update_one(
                    doc! { "_id": starter_oid },
                    doc! { "$inc": { "balance": bet.starter_amount } },
                )
                .session(&mut session)
                .await
                .map_err(|e| {
                    tracing::error!("❌ Failed to refund starter: {}", e);
                    AppError::MongoDB(e)
                })?;

            if let Some(finisher_id) = &bet.finisher_id {
                let finisher_oid = bson::oid::ObjectId::parse_str(finisher_id).map_err(|e| {
                    tracing::error!("❌ Invalid finisher_id: {}", e);
                    AppError::InvalidObjectId(e.to_string())
                })?;
                users_col
                    .update_one(
                        doc! { "_id": finisher_oid },
                        doc! { "$inc": { "balance": bet.finisher_amount.unwrap_or(0.0) } },
                    )
                    .session(&mut session)
                    .await
                    .map_err(|e| {
                        tracing::error!("❌ Failed to refund finisher: {}", e);
                        AppError::MongoDB(e)
                    })?;
            }

            bets_col
                .update_one(
                    doc! { "_id": bet_id },
                    doc! {
                        "$set": {
                            "status": "refunded",
                            "starter_result": "draw",
                            "finisher_result": "draw",
                            "settled_at": now,
                        }
                    },
                )
                .session(&mut session)
                .await
                .map_err(|e| {
                    tracing::error!("❌ Failed to update bet: {}", e);
                    AppError::MongoDB(e)
                })?;

            tracing::info!(
                "🔄 Bet {}: Draw/No winner - refunded both parties",
                bet_id.to_hex()
            );
        }

        session.commit_transaction().await.map_err(|e| {
            tracing::error!("❌ Failed to commit transaction: {}", e);
            AppError::MongoDB(e)
        })?;
        settled_count += 1;
    }

    // ========================================================================
    // PART 2: REFUND UNMATCHED BETS
    // ========================================================================
    let mut open_cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "open",
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find open bets: {}", e);
            AppError::MongoDB(e)
        })?;

    while let Some(bet) = open_cursor.next().await {
        let bet: Bet = bet.map_err(|e| {
            tracing::error!("❌ Failed to deserialize bet: {}", e);
            AppError::MongoDB(e)
        })?;

        tracing::debug!("🔄 Refunding unmatched bet: {:?}", bet.id);

        let mut session = state.client.start_session().await.map_err(|e| {
            tracing::error!("❌ Failed to start session: {}", e);
            AppError::MongoDB(e)
        })?;
        session.start_transaction().await.map_err(|e| {
            tracing::error!("❌ Failed to start transaction: {}", e);
            AppError::MongoDB(e)
        })?;

        let bet_id = bet.id.ok_or(AppError::DocumentNotFound)?;

        let starter_oid = bson::oid::ObjectId::parse_str(&bet.starter_id).map_err(|e| {
            tracing::error!("❌ Invalid starter_id: {}", e);
            AppError::InvalidObjectId(e.to_string())
        })?;
        users_col
            .update_one(
                doc! { "_id": starter_oid },
                doc! { "$inc": { "balance": bet.starter_amount } },
            )
            .session(&mut session)
            .await
            .map_err(|e| {
                tracing::error!("❌ Failed to refund starter: {}", e);
                AppError::MongoDB(e)
            })?;

        bets_col
            .update_one(
                doc! { "_id": bet_id },
                doc! {
                    "$set": {
                        "status": "refunded",
                        "starter_result": "unmatched",
                        "settled_at": now,
                    }
                },
            )
            .session(&mut session)
            .await
            .map_err(|e| {
                tracing::error!("❌ Failed to update bet: {}", e);
                AppError::MongoDB(e)
            })?;

        session.commit_transaction().await.map_err(|e| {
            tracing::error!("❌ Failed to commit transaction: {}", e);
            AppError::MongoDB(e)
        })?;
        refund_count += 1;

        tracing::info!(
            "🔄 Bet {}: Unmatched - refunded starter {} ({} KES)",
            bet_id.to_hex(),
            bet.starter_id,
            bet.starter_amount
        );
    }

    // ========================================================================
    // PART 3: UPDATE FIXTURE STATUS
    // ========================================================================
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
        .map_err(|e| {
            tracing::error!("❌ Failed to update game: {}", e);
            AppError::MongoDB(e)
        })?;

    // Update ALL channel_fixtures with this fixture_id
    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to update channel_fixtures: {}", e);
            AppError::MongoDB(e)
        })?;

    // ========================================================================
    // PART 4: MARK VOTES CORRECT/INCORRECT
    // ========================================================================
    votes_col
        .update_many(
            doc! {
                "fixture_id": &payload.fixture_id,
                "selection": &payload.result,
                "is_correct": null,
            },
            doc! { "$set": { "is_correct": true, "points_awarded": 1 } },
        )
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to mark correct votes: {}", e);
            AppError::MongoDB(e)
        })?;

    votes_col
        .update_many(
            doc! {
                "fixture_id": &payload.fixture_id,
                "selection": { "$ne": &payload.result },
                "is_correct": null,
            },
            doc! { "$set": { "is_correct": false, "points_awarded": 0 } },
        )
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to mark incorrect votes: {}", e);
            AppError::MongoDB(e)
        })?;

    // ========================================================================
    // PART 5: UPDATE CHANNEL MEMBERS
    // ========================================================================
    let mut correct_cursor = votes_col
        .find(doc! { "fixture_id": &payload.fixture_id, "is_correct": true })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find correct votes: {}", e);
            AppError::MongoDB(e)
        })?;
    let mut correct_ids: Vec<String> = Vec::new();
    while let Some(v) = correct_cursor.next().await {
        if let Ok(v) = v {
            correct_ids.push(v.user_id);
        }
    }

    let mut incorrect_cursor = votes_col
        .find(doc! { "fixture_id": &payload.fixture_id, "is_correct": false })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find incorrect votes: {}", e);
            AppError::MongoDB(e)
        })?;
    let mut incorrect_ids: Vec<String> = Vec::new();
    while let Some(v) = incorrect_cursor.next().await {
        if let Ok(v) = v {
            incorrect_ids.push(v.user_id);
        }
    }

    if !correct_ids.is_empty() {
        tracing::debug!("✅ Updating {} correct voters", correct_ids.len());
        channels_col
            .update_many(
                doc! { "members.user_id": { "$in": &correct_ids } },
                doc! { "$inc": {
                    "members.$[m].season_points": 1,
                    "members.$[m].correct_votes": 1,
                    "members.$[m].total_votes": 1,
                }},
            )
            .array_filters(vec![doc! { "m.user_id": { "$in": &correct_ids } }])
            .await
            .map_err(|e| {
                tracing::error!("❌ Failed to update correct voters: {}", e);
                AppError::MongoDB(e)
            })?;
    }

    if !incorrect_ids.is_empty() {
        tracing::debug!("✅ Updating {} incorrect voters", incorrect_ids.len());
        channels_col
            .update_many(
                doc! { "members.user_id": { "$in": &incorrect_ids } },
                doc! { "$inc": { "members.$[m].total_votes": 1 } },
            )
            .array_filters(vec![doc! { "m.user_id": { "$in": &incorrect_ids } }])
            .await
            .map_err(|e| {
                tracing::error!("❌ Failed to update incorrect voters: {}", e);
                AppError::MongoDB(e)
            })?;
    }

    tracing::info!(
        "✅ Settlement complete: {} settled, {} refunded, {} correct, {} incorrect",
        settled_count,
        refund_count,
        correct_ids.len(),
        incorrect_ids.len()
    );

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
// 6. GET FIXTURE VOTERS (from votes collection)
// ============================================================================
pub async fn get_fixture_voters_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    tracing::debug!("📊 Getting voters for fixture: {}", fixture_id);

    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find voters: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut voters = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| {
            tracing::error!("❌ Failed to deserialize vote: {}", e);
            AppError::MongoDB(e)
        })?;
        voters.push(json!({
            "user_id": vote.user_id,
            "user_name": vote.user_name,
            "selection": vote.selection,
            "voted_at": vote.voted_at,
            "is_correct": vote.is_correct,
            "points_awarded": vote.points_awarded,
        }));
    }

    tracing::debug!("✅ Found {} voters", voters.len());

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

    tracing::debug!("📊 Getting votes for user: {}", user_id);

    let mut cursor = votes_col
        .find(doc! { "user_id": &user_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find user votes: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut votes = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| {
            tracing::error!("❌ Failed to deserialize vote: {}", e);
            AppError::MongoDB(e)
        })?;
        votes.push(json!({
            "fixture_id": vote.fixture_id,
            "selection": vote.selection,
            "voted_at": vote.voted_at,
            "is_correct": vote.is_correct,
            "points_awarded": vote.points_awarded,
        }));
    }

    tracing::debug!("✅ Found {} votes for user", votes.len());

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

    tracing::debug!("🔍 Checking vote: fixture={}, user={}", fixture_id, user_id);

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &user_id,
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to check vote: {}", e);
            AppError::MongoDB(e)
        })?;

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

    tracing::debug!(
        "📊 Getting channel vote count: channel={}, fixture={}",
        channel_id,
        fixture_id
    );

    let cf = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel_fixture: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel fixture not found");
            AppError::DocumentNotFound
        })?;

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
// 10. GET VOTE BREAKDOWN (from votes collection)
// ============================================================================
pub async fn get_vote_breakdown_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");

    tracing::debug!("📊 Getting vote breakdown for fixture: {}", fixture_id);

    let pipeline = vec![
        doc! { "$match": { "fixture_id": &fixture_id } },
        doc! { "$group": {
            "_id": "$selection",
            "count": { "$sum": 1 }
        }},
    ];

    let mut cursor = votes_col.aggregate(pipeline).await.map_err(|e| {
        tracing::error!("❌ Failed to aggregate votes: {}", e);
        AppError::MongoDB(e)
    })?;

    let mut breakdown = serde_json::Map::new();
    while let Some(doc) = cursor.next().await {
        let doc = doc.map_err(|e| {
            tracing::error!("❌ Failed to deserialize aggregation: {}", e);
            AppError::MongoDB(e)
        })?;
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

// ============================================================================
// 11. GET CHANNEL VOTES
// ============================================================================
pub async fn get_channel_votes_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    tracing::debug!(
        "📊 Getting channel votes: channel={}, fixture={}",
        channel_id,
        fixture_id
    );

    // Get channel members
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    // Get ALL votes for this fixture
    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find votes: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut channel_votes = Vec::new();
    while let Some(vote) = cursor.next().await {
        let vote: Vote = vote.map_err(|e| {
            tracing::error!("❌ Failed to deserialize vote: {}", e);
            AppError::MongoDB(e)
        })?;
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

    tracing::debug!(
        "📊 Getting channel pledges: channel={}, fixture={}",
        channel_id,
        fixture_id
    );

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": "open",
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find open bets: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut open_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| {
            tracing::error!("❌ Failed to deserialize bet: {}", e);
            AppError::MongoDB(e)
        })?;
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
// 13. GET CHANNEL BETTORS (Matched bets)
// ============================================================================
pub async fn get_channel_bettors_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bets_col: Collection<Bet> = state.db.collection("bets");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    tracing::debug!(
        "📊 Getting channel bettors: channel={}, fixture={}",
        channel_id,
        fixture_id
    );

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

    let member_ids: HashSet<String> = channel.members.iter().map(|m| m.user_id.clone()).collect();

    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &fixture_id,
            "status": { "$in": vec!["matched", "settled"] },
        })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find matched bets: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut matched_bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| {
            tracing::error!("❌ Failed to deserialize bet: {}", e);
            AppError::MongoDB(e)
        })?;
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

    tracing::debug!("📊 Getting channel members: channel={}", channel_id);

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find channel: {}", e);
            AppError::MongoDB(e)
        })?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

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

    tracing::debug!("📊 Getting bets for user: {}", user_id);

    let mut cursor = bets_col
        .find(doc! {
            "$or": [
                { "starter_id": &user_id },
                { "finisher_id": &user_id },
            ]
        })
        .sort(doc! { "created_at": -1 })
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to find user bets: {}", e);
            AppError::MongoDB(e)
        })?;

    let mut bets = Vec::new();
    while let Some(bet) = cursor.next().await {
        let bet: Bet = bet.map_err(|e| {
            tracing::error!("❌ Failed to deserialize bet: {}", e);
            AppError::MongoDB(e)
        })?;
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

    tracing::debug!("✅ Found {} bets for user", bets.len());

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "bets": bets,
        "count": bets.len(),
    })))
}
