use crate::services::cloudinary::CloudinaryService;
use crate::services::fcm_service;
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use axum_extra::extract::Multipart;
use bson::{doc, oid::ObjectId, DateTime as BsonDateTime};
use chrono::{Duration, Utc};
use futures_util::TryStreamExt;
use mongodb::{options::FindOptions, Collection};
use serde_json::json;
use validator::Validate;

use crate::{
    errors::{AppError, Result},
    models::game::Game,
    models::notification::FCMToken,
    models::vote::{
        parse_iso_timestamp_or_now, validate_selection, BulkVoteRequest, BulkVoteResponse, Comment,
        CommentQuery, CommentResponse, CommentStats, CreateComment, CreateLike, CreateVote,
        FixtureCountsResponse, FixtureStats, Like, LikeResponse, LikeStats,
        MarkCommentsSeenRequest, ReplyData, TotalCountsResponse, UserVoteStatus, Vote, VoteQuery,
        VoteResponse, VoteStats,
    },
    state::AppState,
};

// Helper function to get all user IDs from fcm_tokens collection
async fn get_all_user_ids(state: &AppState, exclude_user_id: Option<&str>) -> Vec<String> {
    let tokens_collection: Collection<FCMToken> = state.db.collection("fcm_tokens");

    match tokens_collection.distinct("user_id", doc! {}).await {
        Ok(user_ids_bson) => {
            let mut ids = Vec::new();
            for user_id_bson in user_ids_bson {
                if let Some(user_id_str) = user_id_bson.as_str() {
                    if let Some(exclude) = exclude_user_id {
                        if user_id_str != exclude {
                            ids.push(user_id_str.to_string());
                        }
                    } else {
                        ids.push(user_id_str.to_string());
                    }
                }
            }
            println!("📱 Found {} total users to notify", ids.len());
            ids
        }
        Err(e) => {
            eprintln!("❌ Error getting user IDs: {}", e);
            Vec::new()
        }
    }
}

// ============================================================================
// MEDIA UPLOAD HANDLER (for chat images/videos)
// ============================================================================

pub async fn upload_chat_media(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>> {
    println!("📤 Processing chat media upload...");

    let mut file_data: Vec<u8> = Vec::new();
    let mut file_name = String::new();
    let mut media_type = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                file_name = field.file_name().unwrap_or("upload").to_string();
                match field.bytes().await {
                    Ok(bytes) => {
                        file_data = bytes.to_vec();
                        println!(
                            "📁 Received file: {} ({} bytes)",
                            file_name,
                            file_data.len()
                        );
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to read file data: {}", e);
                        return Err(AppError::ValidationError(format!(
                            "Failed to read file: {}",
                            e
                        )));
                    }
                }
            }
            "type" => match field.text().await {
                Ok(text) => {
                    media_type = text.trim().to_lowercase();
                    println!("📋 Media type: {}", media_type);
                }
                Err(e) => {
                    eprintln!("❌ Failed to read media type: {}", e);
                    return Err(AppError::ValidationError(
                        "Failed to read media type".to_string(),
                    ));
                }
            },
            _ => {}
        }
    }

    if file_data.is_empty() {
        return Err(AppError::ValidationError("No file provided".to_string()));
    }

    if media_type.is_empty() {
        return Err(AppError::ValidationError(
            "Media type (image/video) is required".to_string(),
        ));
    }

    if media_type != "image" && media_type != "video" {
        return Err(AppError::ValidationError(
            "Invalid media type. Must be 'image' or 'video'".to_string(),
        ));
    }

    let max_size = if media_type == "image" {
        10 * 1024 * 1024
    } else {
        50 * 1024 * 1024
    };

    if file_data.len() > max_size {
        return Err(AppError::ValidationError(format!(
            "File too large. Max {}MB for {}",
            max_size / (1024 * 1024),
            media_type
        )));
    }

    let url = match media_type.as_str() {
        "image" => state.cloudinary.upload_image(file_data, &file_name).await,
        "video" => state.cloudinary.upload_video(file_data, &file_name).await,
        _ => unreachable!(),
    };

    match url {
        Ok(secure_url) => {
            println!("✅ Media uploaded to Cloudinary: {}", secure_url);
            Ok(Json(json!({
                "success": true,
                "url": secure_url,
                "secure_url": secure_url,
                "media_type": media_type,
                "message": format!("{} uploaded successfully", media_type)
            })))
        }
        Err(e) => {
            eprintln!("❌ Cloudinary upload failed: {}", e);
            Err(AppError::ValidationError(format!("Upload failed: {}", e)))
        }
    }
}

pub async fn get_comments(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<CommentStats>> {
    get_fixture_comments(State(state), Path(fixture_id)).await
}

pub async fn mark_comments_seen(
    State(state): State<AppState>,
    Json(payload): Json<MarkCommentsSeenRequest>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "👁️ Marking comments as seen for user: {} in fixture: {}",
        payload.user_id, payload.fixture_id
    );

    let collection: Collection<Comment> = state.db.collection("room");

    let filter = if let Some(comment_ids) = &payload.comment_ids {
        let mut object_ids = Vec::new();
        for id_str in comment_ids {
            if let Ok(oid) = ObjectId::parse_str(id_str) {
                object_ids.push(oid);
            }
        }

        doc! {
            "fixtureId": &payload.fixture_id,
            "_id": { "$in": object_ids },
            "seenBy": { "$ne": &payload.user_id }
        }
    } else {
        doc! {
            "fixtureId": &payload.fixture_id,
            "voterId": { "$ne": &payload.user_id },
            "seenBy": { "$ne": &payload.user_id }
        }
    };

    let update = doc! {
        "$addToSet": { "seenBy": &payload.user_id }
    };

    match collection.update_many(filter, update).await {
        Ok(result) => {
            println!("✅ Marked {} comments as seen", result.modified_count);

            let response = serde_json::json!({
                "success": true,
                "message": format!("Marked {} comments as seen", result.modified_count),
                "marked_count": result.modified_count,
                "fixture_id": payload.fixture_id,
                "user_id": payload.user_id,
            });

            Ok(Json(response))
        }
        Err(err) => {
            eprintln!("❌ Error marking comments as seen: {}", err);
            let error_response = serde_json::json!({
                "success": false,
                "message": format!("Failed to mark comments: {}", err),
                "marked_count": 0,
            });
            Ok(Json(error_response))
        }
    }
}

// ========== VOTE HANDLERS ==========

pub async fn create_vote(
    State(state): State<AppState>,
    Json(payload): Json<CreateVote>,
) -> Result<Json<VoteResponse>> {
    println!(
        "🗳️ Creating vote for user: {} ({})",
        payload.username, payload.voter_id
    );

    payload
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    validate_selection(&payload.selection).map_err(|e| AppError::ValidationError(e))?;

    if payload.draw != "draw" {
        return Err(AppError::ValidationError(
            "draw field must be 'draw'".to_string(),
        ));
    }

    if payload.fixture_id.trim().is_empty() {
        return Err(AppError::ValidationError(
            "fixtureId is required".to_string(),
        ));
    }

    let games_collection: Collection<Game> = state.db.collection("games");
    let existing_voter_filter = doc! {
        "match_id": &payload.fixture_id,
        "voters.userId": &payload.voter_id
    };

    let existing_vote_in_game = games_collection.find_one(existing_voter_filter).await?;

    if existing_vote_in_game.is_some() {
        return Ok(Json(VoteResponse {
            success: false,
            message: "User already voted for this fixture".to_string(),
            vote_id: None,
            data: None,
        }));
    }

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let vote = Vote {
        id: None,
        voter_id: payload.voter_id.clone(),
        username: payload.username.clone(),
        fixture_id: payload.fixture_id.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        draw: payload.draw.clone(),
        selection: payload.selection.clone(),
        vote_timestamp: BsonDateTime::from_chrono(Utc::now()),
        created_at: Some(BsonDateTime::from_chrono(Utc::now())),
    };

    let insert_result = vote_collection.insert_one(vote).await?;
    let vote_id = insert_result.inserted_id.as_object_id().unwrap().to_hex();

    let games_update_filter = doc! { "match_id": &payload.fixture_id };
    let games_update = doc! {
        "$inc": { "votes": 1 },
        "$push": {
            "voters": {
                "userId": &payload.voter_id,
                "userName": &payload.username,
                "selection": &payload.selection,
                "votedAt": BsonDateTime::from_chrono(Utc::now())
            }
        }
    };

    match games_collection
        .update_one(games_update_filter, games_update)
        .await
    {
        Ok(result) => {
            println!(
                "✅ Updated games collection: {} modified, votes counter incremented, voter added",
                result.modified_count
            );
        }
        Err(e) => {
            eprintln!("⚠️ Failed to update games collection: {}", e);
        }
    }

    let filter = doc! { "_id": insert_result.inserted_id };
    let inserted_vote = vote_collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    println!(
        "✅ Vote created successfully: {} by {}",
        vote_id, payload.username
    );

    // FCM NOTIFICATIONS
    let state_clone = state.clone();
    let payload_clone = payload.clone();

    tokio::spawn(async move {
        if let Ok(fcm_service) = fcm_service::init_fcm_service().await {
            let all_user_ids = get_all_user_ids(&state_clone, Some(&payload_clone.voter_id)).await;

            let fixture_name =
                format!("{} vs {}", payload_clone.home_team, payload_clone.away_team);
            let vote_text = payload_clone.selection.replace("_", " ");

            if !all_user_ids.is_empty() {
                println!(
                    "📱 Notifying ALL {} users about new vote",
                    all_user_ids.len()
                );
                let _ = fcm_service
                    .send_to_multiple_users(
                        &state_clone,
                        all_user_ids,
                        "⚽ New vote cast!",
                        &format!(
                            "@{} voted {} in {}",
                            payload_clone.username, vote_text, fixture_name
                        ),
                        serde_json::json!({
                            "fixture_id": payload_clone.fixture_id,
                            "voter_id": payload_clone.voter_id,
                            "voter_username": payload_clone.username,
                            "voter_selection": payload_clone.selection,
                            "home_team": payload_clone.home_team,
                            "away_team": payload_clone.away_team,
                            "type": "vote_notification",
                            "action": "new_vote"
                        }),
                        "vote_notification",
                    )
                    .await;
            }
        }
    });

    // WEB SOCKET BROADCAST
    let tx = state.get_or_create_broadcaster(&payload.fixture_id);

    let home_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &payload.fixture_id,
            "selection": "home_team"
        })
        .await
        .unwrap_or(0) as i64;

    let away_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &payload.fixture_id,
            "selection": "away_team"
        })
        .await
        .unwrap_or(0) as i64;

    let draw_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &payload.fixture_id,
            "selection": "draw"
        })
        .await
        .unwrap_or(0) as i64;

    let vote_update = serde_json::json!({
        "type": "vote.update",
        "payload": {
            "fixtureId": payload.fixture_id,
            "userId": payload.voter_id,
            "username": payload.username,
            "selection": payload.selection,
            "home_votes": home_votes,
            "away_votes": away_votes,
            "draw_votes": draw_votes,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(message_json) = serde_json::to_string(&vote_update) {
        let _ = tx.send(message_json);
        println!(
            "📡 Broadcasted vote.update for fixture: {}",
            payload.fixture_id
        );
    }

    Ok(Json(VoteResponse {
        success: true,
        message: "Vote submitted successfully".to_string(),
        vote_id: Some(vote_id),
        data: Some(inserted_vote),
    }))
}

pub async fn get_votes(
    State(state): State<AppState>,
    Query(query): Query<VoteQuery>,
) -> Result<Json<Vec<Vote>>> {
    println!("🔍 Getting votes...");

    let collection: Collection<Vote> = state.db.collection("votes");
    let mut filter = doc! {};

    if let Some(fixture_id) = &query.fixture_id {
        filter.insert("fixtureId", fixture_id);
    }

    if let Some(voter_id) = &query.voter_id {
        filter.insert("voterId", voter_id);
    }

    let options = FindOptions::builder()
        .sort(doc! { "voteTimestamp": -1 })
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let votes: Vec<Vote> = cursor.try_collect().await?;

    println!("✅ Found {} votes", votes.len());
    Ok(Json(votes))
}

pub async fn bulk_create_votes(
    State(state): State<AppState>,
    Json(payload): Json<BulkVoteRequest>,
) -> Result<Json<BulkVoteResponse>> {
    println!("📦 Creating bulk votes ({} votes)", payload.votes.len());

    let collection: Collection<Vote> = state.db.collection("votes");
    let mut failed_votes = Vec::new();
    let mut votes_to_insert = Vec::new();
    let now = BsonDateTime::from_chrono(Utc::now());

    for (index, vote_data) in payload.votes.into_iter().enumerate() {
        match vote_data.validate() {
            Ok(_) => {
                if vote_data.draw != "draw" {
                    failed_votes.push(crate::models::vote::FailedVote {
                        index,
                        error: "draw field must be 'draw'".to_string(),
                        vote_data,
                    });
                    continue;
                }

                if vote_data.fixture_id.trim().is_empty() {
                    failed_votes.push(crate::models::vote::FailedVote {
                        index,
                        error: "fixtureId is required".to_string(),
                        vote_data,
                    });
                    continue;
                }

                let vote = Vote {
                    id: None,
                    voter_id: vote_data.voter_id.clone(),
                    username: vote_data.username.clone(),
                    fixture_id: vote_data.fixture_id.clone(),
                    home_team: vote_data.home_team.clone(),
                    away_team: vote_data.away_team.clone(),
                    draw: vote_data.draw.clone(),
                    selection: vote_data.selection.clone(),
                    vote_timestamp: now,
                    created_at: Some(now),
                };

                votes_to_insert.push(vote);
            }
            Err(e) => {
                failed_votes.push(crate::models::vote::FailedVote {
                    index,
                    error: e.to_string(),
                    vote_data,
                });
            }
        }
    }

    let inserted_count = if !votes_to_insert.is_empty() {
        let result = collection.insert_many(votes_to_insert).await?;
        result.inserted_ids.len() as u64
    } else {
        0
    };

    let failed_count = failed_votes.len() as u64;

    println!(
        "✅ Bulk vote creation: {} inserted, {} failed",
        inserted_count, failed_count
    );

    Ok(Json(BulkVoteResponse {
        success: true,
        inserted_count,
        failed_count,
        failed_votes,
    }))
}

pub async fn get_user_unread_counts(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("🔔 Getting unread comment counts for user: {}", user_id);

    let collection: Collection<Comment> = state.db.collection("room");

    let pipeline = vec![
        doc! {
            "$match": {
                "voterId": { "$ne": &user_id },
                "seenBy": { "$ne": &user_id }
            }
        },
        doc! {
            "$group": {
                "_id": "$fixtureId",
                "count": { "$sum": 1 }
            }
        },
    ];

    let mut cursor = collection.aggregate(pipeline).await?;
    let mut counts = std::collections::HashMap::new();

    while let Some(doc) = cursor.try_next().await? {
        let fixture_id = doc
            .get("_id")
            .and_then(|id| id.as_str())
            .map(|s| s.to_string());

        let count = doc.get("count").and_then(|c| c.as_i64()).unwrap_or(0);

        if let Some(id) = fixture_id {
            counts.insert(id, count);
        }
    }

    let response = json!({
        "success": true,
        "user_id": user_id,
        "unread_counts": counts,
    });

    Ok(Json(response))
}

// ========== CORRECTED: get_user_votes - QUERIES GAMES COLLECTION ==========
pub async fn get_user_votes(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<Vote>>> {
    println!("🔍 Looking for user_id: '{}'", user_id);

    let games_collection: Collection<Game> = state.db.collection("games");

    // First, let's see all games with voters
    let all_games_with_voters = games_collection
        .find(doc! { "voters": { "$exists": true, "$ne": [] } })
        .await?;
    let games_list: Vec<Game> = all_games_with_voters.try_collect().await?;
    println!("📊 Total games with voters: {}", games_list.len());

    for game in &games_list {
        println!(
            "  Game: {} vs {}, voters count: {}",
            game.home_team,
            game.away_team,
            game.voters.len()
        );
        for voter in &game.voters {
            println!(
                "    Voter: userId='{}', userName='{}'",
                voter.user_id, voter.user_name
            );
        }
    }

    // Now query for specific user
    let filter = doc! { "voters.userId": &user_id };
    println!("🔍 Query filter: {:?}", filter);

    let cursor = games_collection.find(filter).await?;
    let games: Vec<Game> = cursor.try_collect().await?;

    println!("📊 Found {} games with votes from this user", games.len());

    let mut votes = Vec::new();

    for game in games {
        for voter in &game.voters {
            if voter.user_id == user_id {
                println!(
                    "✅ Found vote: {} voted {} in {} vs {}",
                    voter.user_name, voter.selection, game.home_team, game.away_team
                );

                let vote = Vote {
                    id: None,
                    voter_id: voter.user_id.clone(),
                    username: voter.user_name.clone(),
                    fixture_id: game.match_id.clone(),
                    home_team: game.home_team.clone(),
                    away_team: game.away_team.clone(),
                    draw: "draw".to_string(),
                    selection: voter.selection.clone(),
                    vote_timestamp: voter.voted_at,
                    created_at: Some(voter.voted_at),
                };
                votes.push(vote);
            }
        }
    }

    println!("✅ Returning {} votes for user", votes.len());
    Ok(Json(votes))
}
pub async fn get_fixture_votes(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<VoteStats>> {
    println!("📊 Getting vote stats for fixture: {}", fixture_id);

    let collection: Collection<Vote> = state.db.collection("votes");
    let filter = doc! { "fixtureId": &fixture_id };

    let cursor = collection.find(filter).await?;
    let votes: Vec<Vote> = cursor.try_collect().await?;

    let total_votes = votes.len() as i64;
    let home_votes = votes.iter().filter(|v| v.selection == "home_team").count() as i64;
    let draw_votes = votes.iter().filter(|v| v.selection == "draw").count() as i64;
    let away_votes = votes.iter().filter(|v| v.selection == "away_team").count() as i64;

    let (home_team, away_team) = if let Some(first_vote) = votes.first() {
        (first_vote.home_team.clone(), first_vote.away_team.clone())
    } else {
        ("Unknown".to_string(), "Unknown".to_string())
    };

    let home_percentage = if total_votes > 0 {
        (home_votes as f64 / total_votes as f64) * 100.0
    } else {
        0.0
    };

    let draw_percentage = if total_votes > 0 {
        (draw_votes as f64 / total_votes as f64) * 100.0
    } else {
        0.0
    };

    let away_percentage = if total_votes > 0 {
        (away_votes as f64 / total_votes as f64) * 100.0
    } else {
        0.0
    };

    let stats = VoteStats {
        fixture_id: fixture_id.clone(),
        home_team,
        away_team,
        total_votes,
        home_votes,
        draw_votes,
        away_votes,
        home_percentage,
        draw_percentage,
        away_percentage,
    };

    println!("✅ Vote stats: {} votes total", total_votes);
    Ok(Json(stats))
}

pub async fn get_total_votes_for_fixture(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting total vote count for fixture: {}", fixture_id);

    let collection: Collection<Vote> = state.db.collection("votes");
    let filter = doc! { "fixtureId": &fixture_id };

    let total_votes = collection.count_documents(filter).await? as i64;

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "total_votes": total_votes,
        "timestamp": Utc::now().to_rfc3339(),
    });

    println!("✅ Total votes for fixture {}: {}", fixture_id, total_votes);
    Ok(Json(response))
}

pub async fn get_user_vote_for_fixture(
    State(state): State<AppState>,
    Path((fixture_id, voter_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "🔍 Checking user vote: {} for fixture: {}",
        voter_id, fixture_id
    );

    let games_collection: Collection<Game> = state.db.collection("games");
    let filter = doc! {
        "match_id": &fixture_id,
        "voters.userId": &voter_id,
    };

    let game = games_collection.find_one(filter).await?;
    let has_voted = game.is_some();

    let mut selection = None;
    if let Some(g) = game {
        for voter in g.voters {
            if voter.user_id == voter_id {
                selection = Some(voter.selection);
                break;
            }
        }
    }

    println!("✅ User has voted: {}", has_voted);

    Ok(Json(json!({
        "hasVoted": has_voted,
        "selection": selection,
    })))
}

pub async fn get_vote_counts_by_selection(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "📊 Getting vote counts by selection for fixture: {}",
        fixture_id
    );

    let collection: Collection<Vote> = state.db.collection("votes");

    let home_votes = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_votes = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_votes = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let total_votes = home_votes + draw_votes + away_votes;

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "vote_counts": {
            "home_team": home_votes,
            "draw": draw_votes,
            "away_team": away_votes,
            "total": total_votes
        },
        "percentages": {
            "home": if total_votes > 0 { (home_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 },
            "draw": if total_votes > 0 { (draw_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 },
            "away": if total_votes > 0 { (away_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 }
        },
        "timestamp": Utc::now().to_rfc3339()
    });

    println!(
        "✅ Vote counts by selection for fixture {}: H:{} D:{} A:{}",
        fixture_id, home_votes, draw_votes, away_votes
    );
    Ok(Json(response))
}

pub async fn delete_vote(
    State(state): State<AppState>,
    Path(vote_id): Path<String>,
) -> Result<Json<VoteResponse>> {
    println!("🗑️ Deleting vote: {}", vote_id);

    let collection: Collection<Vote> = state.db.collection("votes");

    let object_id = ObjectId::parse_str(&vote_id)
        .map_err(|_| AppError::invalid_data("Invalid vote ID format"))?;

    let filter = doc! { "_id": object_id };

    let delete_result = collection.delete_one(filter).await?;

    if delete_result.deleted_count == 0 {
        return Ok(Json(VoteResponse {
            success: false,
            message: "Vote not found".to_string(),
            vote_id: None,
            data: None,
        }));
    }

    println!("✅ Vote deleted successfully");
    Ok(Json(VoteResponse {
        success: true,
        message: "Vote deleted successfully".to_string(),
        vote_id: Some(vote_id),
        data: None,
    }))
}

// ========== LIKE HANDLERS ==========

pub async fn create_like(
    State(state): State<AppState>,
    Json(payload): Json<CreateLike>,
) -> Result<Json<LikeResponse>> {
    println!(
        "👍 Creating like for user: {} ({})",
        payload.username, payload.voter_id
    );

    payload
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let collection: Collection<Like> = state.db.collection("likes");
    let existing_like_filter = doc! {
        "voterId": &payload.voter_id,
        "fixtureId": &payload.fixture_id,
    };

    let existing_like = collection.find_one(existing_like_filter.clone()).await?;
    let total_likes: i64;
    let message: String;
    let success: bool;
    let mut like_id: Option<String> = None;

    if let Some(_like) = existing_like {
        if payload.action == "unlike" {
            collection.delete_one(existing_like_filter).await?;
            let fixture_filter = doc! { "fixtureId": &payload.fixture_id };
            total_likes = collection.count_documents(fixture_filter).await? as i64;
            message = "Like removed successfully".to_string();
            success = true;
            println!(
                "👎 Like removed for fixture: {} by {}",
                payload.fixture_id, payload.username
            );
        } else {
            return Ok(Json(LikeResponse {
                success: false,
                message: "User already liked this fixture".to_string(),
                like_id: None,
                total_likes: 0,
            }));
        }
    } else {
        if payload.action != "like" {
            return Ok(Json(LikeResponse {
                success: false,
                message: "Cannot unlike a fixture you haven't liked".to_string(),
                like_id: None,
                total_likes: 0,
            }));
        }

        let like = Like {
            id: None,
            voter_id: payload.voter_id.clone(),
            username: payload.username.clone(),
            fixture_id: payload.fixture_id.clone(),
            action: payload.action.clone(),
            like_timestamp: BsonDateTime::from_chrono(Utc::now()),
            created_at: Some(BsonDateTime::from_chrono(Utc::now())),
        };

        let insert_result = collection.insert_one(like).await?;
        let fixture_filter = doc! { "fixtureId": &payload.fixture_id };
        total_likes = collection.count_documents(fixture_filter).await? as i64;
        message = "Like added successfully".to_string();
        success = true;

        if let Some(id) = insert_result.inserted_id.as_object_id() {
            like_id = Some(id.to_hex());
        }

        println!(
            "✅ Like created for fixture: {} by {}",
            payload.fixture_id, payload.username
        );

        // FCM NOTIFICATIONS
        let state_clone = state.clone();
        let payload_clone = payload.clone();

        tokio::spawn(async move {
            if let Ok(fcm_service) = fcm_service::init_fcm_service().await {
                let all_user_ids =
                    get_all_user_ids(&state_clone, Some(&payload_clone.voter_id)).await;

                let games_collection: Collection<Game> = state_clone.db.collection("games");
                let game_filter = doc! { "match_id": &payload_clone.fixture_id };

                let (home_team, away_team) = match games_collection.find_one(game_filter).await {
                    Ok(Some(game)) => (game.home_team.clone(), game.away_team.clone()),
                    _ => ("Unknown".to_string(), "Unknown".to_string()),
                };

                let fixture_name = format!("{} vs {}", home_team, away_team);

                if !all_user_ids.is_empty() {
                    println!(
                        "📱 Notifying ALL {} users about new like",
                        all_user_ids.len()
                    );
                    let _ = fcm_service
                        .send_to_multiple_users(
                            &state_clone,
                            all_user_ids,
                            "❤️ New like on a fixture!",
                            &format!("@{} liked {}", payload_clone.username, fixture_name),
                            serde_json::json!({
                                "fixture_id": payload_clone.fixture_id,
                                "liker_id": payload_clone.voter_id,
                                "liker_username": payload_clone.username,
                                "home_team": home_team,
                                "away_team": away_team,
                                "type": "like_notification",
                                "action": "new_like"
                            }),
                            "like_notification",
                        )
                        .await;
                }
            }
        });

        // WEB SOCKET BROADCAST
        let tx = state.get_or_create_broadcaster(&payload.fixture_id);

        let like_update = serde_json::json!({
            "type": "like",
            "payload": {
                "fixtureId": payload.fixture_id,
                "userId": payload.voter_id,
                "username": payload.username,
                "totalLikes": total_likes,
            },
            "timestamp": Utc::now().to_rfc3339(),
        });

        if let Ok(message_json) = serde_json::to_string(&like_update) {
            let _ = tx.send(message_json);
            println!("📡 Broadcasted like for fixture: {}", payload.fixture_id);
        }
    }

    Ok(Json(LikeResponse {
        success,
        message,
        like_id,
        total_likes,
    }))
}

pub async fn get_fixture_likes(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<LikeStats>> {
    println!("👍 Getting likes for fixture: {}", fixture_id);

    let collection: Collection<Like> = state.db.collection("likes");
    let filter = doc! { "fixtureId": &fixture_id };
    let total_likes = collection.count_documents(filter).await? as i64;

    let stats = LikeStats {
        fixture_id: fixture_id.clone(),
        total_likes,
        user_has_liked: false,
    };

    println!("✅ Found {} likes for fixture", total_likes);
    Ok(Json(stats))
}

pub async fn get_total_likes_for_fixture(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("👍 Getting total like count for fixture: {}", fixture_id);

    let collection: Collection<Like> = state.db.collection("likes");
    let filter = doc! { "fixtureId": &fixture_id };
    let total_likes = collection.count_documents(filter).await? as i64;

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "total_likes": total_likes,
        "timestamp": Utc::now().to_rfc3339(),
    });

    println!("✅ Total likes for fixture {}: {}", fixture_id, total_likes);
    Ok(Json(response))
}

pub async fn get_user_like_for_fixture(
    State(state): State<AppState>,
    Path((fixture_id, voter_id)): Path<(String, String)>,
) -> Result<Json<LikeStats>> {
    println!(
        "👍 Checking user like: {} for fixture: {}",
        voter_id, fixture_id
    );

    let collection: Collection<Like> = state.db.collection("likes");

    let fixture_filter = doc! { "fixtureId": &fixture_id };
    let total_likes = collection.count_documents(fixture_filter).await? as i64;

    let user_like_filter = doc! {
        "fixtureId": &fixture_id,
        "voterId": &voter_id,
    };
    let user_has_liked = collection.find_one(user_like_filter).await?.is_some();

    let stats = LikeStats {
        fixture_id: fixture_id.clone(),
        total_likes,
        user_has_liked,
    };

    println!("✅ User {} has liked: {}", voter_id, user_has_liked);
    Ok(Json(stats))
}

pub async fn delete_like(
    State(state): State<AppState>,
    Path(like_id): Path<String>,
) -> Result<Json<LikeResponse>> {
    println!("🗑️ Deleting like: {}", like_id);

    let collection: Collection<Like> = state.db.collection("likes");

    let object_id = ObjectId::parse_str(&like_id)
        .map_err(|_| AppError::invalid_data("Invalid like ID format"))?;

    let filter = doc! { "_id": object_id };

    let like = collection
        .find_one(filter.clone())
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    let delete_result = collection.delete_one(filter).await?;

    if delete_result.deleted_count == 0 {
        return Ok(Json(LikeResponse {
            success: false,
            message: "Like not found".to_string(),
            like_id: None,
            total_likes: 0,
        }));
    }

    let fixture_filter = doc! { "fixtureId": &like.fixture_id };
    let total_likes = collection.count_documents(fixture_filter).await? as i64;

    println!("✅ Like deleted successfully");
    Ok(Json(LikeResponse {
        success: true,
        message: "Like deleted successfully".to_string(),
        like_id: Some(like_id),
        total_likes,
    }))
}

// ========== COMMENT HANDLERS ==========

pub async fn create_comment(
    State(state): State<AppState>,
    Json(payload): Json<CreateComment>,
) -> Result<Json<CommentResponse>> {
    println!(
        "💬 Creating comment for user: {} ({})",
        payload.username, payload.voter_id
    );

    payload
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    if payload.selection.trim().is_empty() {
        return Err(AppError::ValidationError(
            "selection is required".to_string(),
        ));
    }

    validate_selection(&payload.selection).map_err(|e| AppError::ValidationError(e))?;

    if payload.fixture_id.trim().is_empty() {
        return Err(AppError::ValidationError(
            "fixtureId is required".to_string(),
        ));
    }

    let has_content = !payload.comment.is_empty() || payload.is_image || payload.is_video;
    if !has_content {
        return Err(AppError::ValidationError(
            "Comment, image, or video is required".to_string(),
        ));
    }

    let collection: Collection<Comment> = state.db.collection("room");
    let comment_timestamp = parse_iso_timestamp_or_now(&payload.timestamp);

    let comment = Comment {
        id: None,
        voter_id: payload.voter_id.clone(),
        username: payload.username.clone(),
        fixture_id: payload.fixture_id.clone(),
        selection: payload.selection.clone(),
        comment: payload.comment.clone(),
        timestamp: payload.timestamp.clone(),
        comment_timestamp,
        created_at: Some(BsonDateTime::from_chrono(Utc::now())),
        likes: Some(0),
        replies: Some(Vec::new()),
        seen_by: vec![],
        image_url: payload.image_url.clone(),
        video_url: payload.video_url.clone(),
        is_image: payload.is_image,
        is_video: payload.is_video,
        reply_to: payload.reply_to.clone(),
        message_id: payload.message_id.clone(),
    };

    let insert_result = collection.insert_one(comment).await?;
    let comment_id = insert_result.inserted_id.as_object_id().unwrap().to_hex();
    let comment_id_for_closure = comment_id.clone();

    let filter = doc! { "_id": insert_result.inserted_id };
    let inserted_comment = collection
        .find_one(filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    println!(
        "✅ Comment created successfully: {} by {}",
        comment_id, payload.username
    );

    let games_collection: Collection<Game> = state.db.collection("games");
    let games_update_filter = doc! { "match_id": &payload.fixture_id };
    let games_update = doc! { "$inc": { "comments": 1 } };
    let _ = games_collection
        .update_one(games_update_filter, games_update)
        .await;

    let game_filter = doc! { "match_id": &payload.fixture_id };
    let game = games_collection.find_one(game_filter).await;
    let (home_team, away_team) = match game {
        Ok(Some(g)) => (g.home_team, g.away_team),
        _ => ("Unknown".to_string(), "Unknown".to_string()),
    };

    // FCM NOTIFICATIONS
    let state_clone = state.clone();
    let payload_clone = payload.clone();
    let comment_text = payload.comment.clone();
    let fixture_name = format!("{} vs {}", home_team, away_team);

    // ✅ FIXED: Safe Unicode/emoji truncation - counts characters, not bytes
    let short_comment = if comment_text.chars().count() > 50 {
        let truncated: String = comment_text.chars().take(50).collect();
        format!("{}...", truncated)
    } else {
        comment_text.clone()
    };

    let commenter_name = payload_clone.username.clone();
    let has_image = payload_clone.is_image;
    let has_video = payload_clone.is_video;
    let reply_to = payload_clone.reply_to.clone();

    tokio::spawn(async move {
        if let Ok(fcm_service) = fcm_service::init_fcm_service().await {
            let all_user_ids = get_all_user_ids(&state_clone, Some(&payload_clone.voter_id)).await;

            if !all_user_ids.is_empty() {
                let media_emoji = if has_image {
                    "📷 "
                } else if has_video {
                    "🎥 "
                } else {
                    ""
                };
                let reply_text = if let Some(reply) = reply_to {
                    format!(" (replying to @{})", reply.username)
                } else {
                    "".to_string()
                };

                let notification_payload = serde_json::json!({
                    "fixture_id": payload_clone.fixture_id,
                    "comment_id": comment_id_for_closure,
                    "voter_id": payload_clone.voter_id,
                    "voter_username": payload_clone.username,
                    "voter_selection": payload_clone.selection,
                    "comment": comment_text,
                    "comment_preview": short_comment,
                    "imageUrl": payload_clone.image_url,
                    "videoUrl": payload_clone.video_url,
                    "isImage": payload_clone.is_image,
                    "isVideo": payload_clone.is_video,
                    "replyTo": payload_clone.reply_to,
                    "home_team": home_team,
                    "away_team": away_team,
                    "fixture_name": fixture_name,
                    "type": "comment_notification",
                    "action": "new_comment",
                    "timestamp": Utc::now().to_rfc3339(),
                });

                let _ = fcm_service
                    .send_to_multiple_users(
                        &state_clone,
                        all_user_ids,
                        &format!("💬 @{}{}{}", commenter_name, media_emoji, reply_text),
                        &format!("\"{}\" on {}", short_comment, fixture_name),
                        notification_payload,
                        "comment_notification",
                    )
                    .await;
            }
        }
    });

    // WEB SOCKET BROADCASTS
    let tx = state.get_or_create_broadcaster(&payload.fixture_id);

    let chat_message = serde_json::json!({
        "type": "chat.message",
        "payload": {
            "comment_id": comment_id,
            "voter_id": payload.voter_id,
            "username": payload.username,
            "fixture_id": payload.fixture_id,
            "selection": payload.selection,
            "comment": payload.comment,
            "imageUrl": payload.image_url,
            "videoUrl": payload.video_url,
            "isImage": payload.is_image,
            "isVideo": payload.is_video,
            "replyTo": payload.reply_to,
            "timestamp": Utc::now().to_rfc3339(),
            "likes": 0,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(message_json) = serde_json::to_string(&chat_message) {
        let _ = tx.send(message_json);
        println!(
            "📡 Broadcasted chat.message for fixture: {}",
            payload.fixture_id
        );
    }

    let fixture_comment = serde_json::json!({
        "type": "fixture.comment",
        "payload": {
            "fixtureId": payload.fixture_id,
            "comment": payload.comment,
            "username": payload.username,
            "selection": payload.selection,
            "commentId": comment_id,
            "timestamp": Utc::now().to_rfc3339(),
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(message_json) = serde_json::to_string(&fixture_comment) {
        let _ = tx.send(message_json);
        println!(
            "📡 Broadcasted fixture.comment for fixture: {}",
            payload.fixture_id
        );
    }

    let comment_collection: Collection<Comment> = state.db.collection("room");
    let comment_filter = doc! { "fixtureId": &payload.fixture_id };
    let total_comments = match comment_collection.count_documents(comment_filter).await {
        Ok(count) => count as i64,
        Err(_) => 0,
    };

    let comment_count_update = serde_json::json!({
        "type": "comment.count",
        "payload": {
            "fixtureId": payload.fixture_id,
            "count": total_comments,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(message_json) = serde_json::to_string(&comment_count_update) {
        let _ = tx.send(message_json);
        println!(
            "📡 Broadcasted comment.count for fixture: {} (total: {})",
            payload.fixture_id, total_comments
        );
    }

    Ok(Json(CommentResponse {
        success: true,
        message: "Comment submitted successfully".to_string(),
        comment_id: Some(comment_id),
        comment: Some(inserted_comment),
    }))
}

pub async fn get_fixture_comments(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<CommentStats>> {
    println!("💬 Getting comments for fixture: {}", fixture_id);

    let collection: Collection<Comment> = state.db.collection("room");
    let filter = doc! { "fixtureId": &fixture_id };

    let options = FindOptions::builder()
        .sort(doc! { "commentTimestamp": -1 })
        .limit(20)
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let all_comments: Vec<Comment> = cursor.try_collect().await?;

    let total_comments = all_comments.len() as i64;
    let home_comments = all_comments
        .iter()
        .filter(|c| c.selection == "home_team")
        .count() as i64;
    let draw_comments = all_comments
        .iter()
        .filter(|c| c.selection == "draw")
        .count() as i64;
    let away_comments = all_comments
        .iter()
        .filter(|c| c.selection == "away_team")
        .count() as i64;
    let recent_comments: Vec<Comment> = all_comments.into_iter().take(10).collect();

    let stats = CommentStats {
        fixture_id: fixture_id.clone(),
        total_comments,
        home_comments,
        draw_comments,
        away_comments,
        recent_comments,
    };

    println!(
        "✅ Found {} comments for fixture (H:{} D:{} A:{})",
        total_comments, home_comments, draw_comments, away_comments
    );
    Ok(Json(stats))
}

pub async fn get_total_comments_for_fixture(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("💬 Getting total comment count for fixture: {}", fixture_id);

    let collection: Collection<Comment> = state.db.collection("room");
    let filter = doc! { "fixtureId": &fixture_id };
    let total_comments = collection.count_documents(filter).await? as i64;

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "total_comments": total_comments,
        "timestamp": Utc::now().to_rfc3339(),
    });

    println!(
        "✅ Total comments for fixture {}: {}",
        fixture_id, total_comments
    );
    Ok(Json(response))
}

pub async fn get_user_comments(
    State(state): State<AppState>,
    Path(voter_id): Path<String>,
) -> Result<Json<Vec<Comment>>> {
    println!("🔍 Getting comments for user: {}", voter_id);

    let collection: Collection<Comment> = state.db.collection("room");
    let filter = doc! { "voterId": voter_id };

    let options = FindOptions::builder()
        .sort(doc! { "commentTimestamp": -1 })
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let comments: Vec<Comment> = cursor.try_collect().await?;

    println!("✅ Found {} comments for user", comments.len());
    Ok(Json(comments))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
) -> Result<Json<CommentResponse>> {
    println!("🗑️ Deleting comment: {}", comment_id);

    let collection: Collection<Comment> = state.db.collection("room");

    let object_id = ObjectId::parse_str(&comment_id)
        .map_err(|_| AppError::invalid_data("Invalid comment ID format"))?;

    let filter = doc! { "_id": object_id };

    let delete_result = collection.delete_one(filter).await?;

    if delete_result.deleted_count == 0 {
        return Ok(Json(CommentResponse {
            success: false,
            message: "Comment not found".to_string(),
            comment_id: None,
            comment: None,
        }));
    }

    println!("✅ Comment deleted successfully");
    Ok(Json(CommentResponse {
        success: true,
        message: "Comment deleted successfully".to_string(),
        comment_id: Some(comment_id),
        comment: None,
    }))
}

pub async fn like_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
) -> Result<Json<CommentResponse>> {
    println!("👍 Liking comment: {}", comment_id);

    let collection: Collection<Comment> = state.db.collection("room");

    let object_id = ObjectId::parse_str(&comment_id)
        .map_err(|_| AppError::invalid_data("Invalid comment ID format"))?;

    let filter = doc! { "_id": object_id };
    let update = doc! { "$inc": { "likes": 1 } };

    let update_result = collection.update_one(filter, update).await?;

    if update_result.matched_count == 0 {
        return Ok(Json(CommentResponse {
            success: false,
            message: "Comment not found".to_string(),
            comment_id: None,
            comment: None,
        }));
    }

    let updated_filter = doc! { "_id": object_id };
    let updated_comment = collection
        .find_one(updated_filter)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    println!("✅ Comment liked successfully");
    Ok(Json(CommentResponse {
        success: true,
        message: "Comment liked successfully".to_string(),
        comment_id: Some(comment_id),
        comment: Some(updated_comment),
    }))
}

// ========== STATISTICS HANDLERS ==========

pub async fn get_vote_stats(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<VoteStats>> {
    get_fixture_votes(State(state), Path(fixture_id)).await
}

pub async fn get_like_stats(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<LikeStats>> {
    get_fixture_likes(State(state), Path(fixture_id)).await
}

pub async fn get_comment_stats(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<CommentStats>> {
    get_fixture_comments(State(state), Path(fixture_id)).await
}

pub async fn get_fixture_stats(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<FixtureStats>> {
    println!("📊 Getting comprehensive stats for fixture: {}", fixture_id);

    let vote_stats = get_vote_stats(State(state.clone()), Path(fixture_id.clone()))
        .await?
        .0;

    let like_stats = get_like_stats(State(state.clone()), Path(fixture_id.clone()))
        .await?
        .0;

    let comment_stats = get_comment_stats(State(state.clone()), Path(fixture_id.clone()))
        .await?
        .0;

    let stats = FixtureStats {
        fixture_id: fixture_id.clone(),
        home_team: vote_stats.home_team.clone(),
        away_team: vote_stats.away_team.clone(),
        vote_stats,
        like_stats,
        comment_stats,
    };

    println!("✅ Comprehensive stats generated for fixture");
    Ok(Json(stats))
}

pub async fn get_all_counts_for_fixture(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<FixtureCountsResponse>> {
    println!("📊 Getting all counts for fixture: {}", fixture_id);

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let vote_filter = doc! { "fixtureId": &fixture_id };
    let total_votes = vote_collection.count_documents(vote_filter.clone()).await? as i64;

    let home_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let like_filter = doc! { "fixtureId": &fixture_id };
    let total_likes = like_collection.count_documents(like_filter).await? as i64;

    let comment_filter = doc! { "fixtureId": &fixture_id };
    let total_comments = comment_collection.count_documents(comment_filter).await? as i64;

    let first_vote = vote_collection.find_one(vote_filter).await?;
    let (home_team, away_team) = if let Some(vote) = first_vote {
        (vote.home_team.clone(), vote.away_team.clone())
    } else {
        ("Unknown".to_string(), "Unknown".to_string())
    };

    let total_engagement = total_votes + total_likes + total_comments;

    let counts = crate::models::vote::FixtureCounts {
        fixture_id: fixture_id.clone(),
        home_team,
        away_team,
        total_votes,
        home_votes,
        draw_votes,
        away_votes,
        total_likes,
        total_comments,
        total_engagement,
        user_has_voted: false,
        user_has_liked: false,
        user_selection: None,
    };

    let response = FixtureCountsResponse {
        success: true,
        message: format!("Counts retrieved for fixture {}", fixture_id),
        data: counts,
    };

    println!(
        "✅ All counts for fixture {}: {} votes, {} likes, {} comments",
        fixture_id, total_votes, total_likes, total_comments
    );
    Ok(Json(response))
}

pub async fn get_user_stats(
    State(state): State<AppState>,
    Path(voter_id): Path<String>,
) -> Result<Json<UserVoteStatus>> {
    println!("👤 Getting stats for user: {}", voter_id);

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let vote_filter = doc! { "voterId": &voter_id };
    let votes_count = vote_collection.count_documents(vote_filter).await? as i64;

    let like_collection: Collection<Like> = state.db.collection("likes");
    let like_filter = doc! { "voterId": &voter_id };
    let likes_count = like_collection.count_documents(like_filter).await? as i64;

    let comment_collection: Collection<Comment> = state.db.collection("room");
    let comment_filter = doc! { "voterId": &voter_id };
    let comments_count = comment_collection.count_documents(comment_filter).await? as i64;

    let stats = UserVoteStatus {
        fixture_id: "all".to_string(),
        has_voted: votes_count > 0,
        vote_selection: None,
        has_liked: likes_count > 0,
        user_comments_count: comments_count,
    };

    println!(
        "✅ User stats: {} votes, {} likes, {} comments",
        votes_count, likes_count, comments_count
    );
    Ok(Json(stats))
}

pub async fn get_total_counts(State(state): State<AppState>) -> Result<Json<TotalCountsResponse>> {
    println!("📈 Getting total counts across all fixtures");

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let total_votes = vote_collection.estimated_document_count().await? as i64;
    let total_likes = like_collection.estimated_document_count().await? as i64;
    let total_comments = comment_collection.estimated_document_count().await? as i64;

    let unique_users = vote_collection.distinct("voterId", doc! {}).await?.len() as i64;
    let total_engagement = total_votes + total_likes + total_comments;

    let counts = crate::models::vote::TotalCounts {
        total_votes,
        total_likes,
        total_comments,
        total_engagement,
        total_users: unique_users,
        timestamp: Utc::now().to_rfc3339(),
    };

    let response = TotalCountsResponse {
        success: true,
        message: "Total counts retrieved successfully".to_string(),
        data: counts,
    };

    println!(
        "✅ Total counts: {} votes, {} likes, {} comments, {} users",
        total_votes, total_likes, total_comments, unique_users
    );
    Ok(Json(response))
}

pub async fn get_batch_fixture_counts(
    State(state): State<AppState>,
    Json(payload): Json<crate::models::vote::BatchFixtureCountsRequest>,
) -> Result<Json<crate::models::vote::BatchFixtureCountsResponse>> {
    println!(
        "📊 Getting batch counts for {} fixtures",
        payload.fixture_ids.len()
    );

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let mut fixture_counts = Vec::new();

    for fixture_id in payload.fixture_ids {
        let vote_filter = doc! { "fixtureId": &fixture_id };
        let total_votes = vote_collection.count_documents(vote_filter.clone()).await? as i64;

        let home_votes = vote_collection
            .count_documents(doc! {
                "fixtureId": &fixture_id,
                "selection": "home_team"
            })
            .await? as i64;

        let draw_votes = vote_collection
            .count_documents(doc! {
                "fixtureId": &fixture_id,
                "selection": "draw"
            })
            .await? as i64;

        let away_votes = vote_collection
            .count_documents(doc! {
                "fixtureId": &fixture_id,
                "selection": "away_team"
            })
            .await? as i64;

        let like_filter = doc! { "fixtureId": &fixture_id };
        let total_likes = like_collection.count_documents(like_filter).await? as i64;

        let comment_filter = doc! { "fixtureId": &fixture_id };
        let total_comments = comment_collection.count_documents(comment_filter).await? as i64;

        let first_vote = vote_collection.find_one(vote_filter).await?;
        let (home_team, away_team) = if let Some(vote) = first_vote {
            (vote.home_team.clone(), vote.away_team.clone())
        } else {
            ("Unknown".to_string(), "Unknown".to_string())
        };

        let total_engagement = total_votes + total_likes + total_comments;

        let mut user_has_voted = None;
        let mut user_has_liked = None;
        let mut user_selection = None;

        if let Some(user_id) = &payload.user_id {
            let user_vote_filter = doc! {
                "fixtureId": &fixture_id,
                "voterId": user_id,
            };
            let user_vote = vote_collection.find_one(user_vote_filter).await?;
            user_has_voted = Some(user_vote.is_some());
            user_selection = user_vote.map(|v| v.selection);

            let user_like_filter = doc! {
                "fixtureId": &fixture_id,
                "voterId": user_id,
            };
            let user_like = like_collection.find_one(user_like_filter).await?;
            user_has_liked = Some(user_like.is_some());
        }

        let count_item = crate::models::vote::FixtureCountItem {
            fixture_id: fixture_id.clone(),
            home_team,
            away_team,
            total_votes,
            total_likes,
            total_comments,
            total_engagement,
            user_has_voted,
            user_has_liked,
            user_selection,
        };

        fixture_counts.push(count_item);
    }

    let count = fixture_counts.len();

    let response = crate::models::vote::BatchFixtureCountsResponse {
        success: true,
        message: format!("Counts retrieved for {} fixtures", count),
        data: fixture_counts,
        count,
    };

    println!("✅ Batch counts retrieved for {} fixtures", count);
    Ok(Json(response))
}

// ========== ADMIN HANDLERS ==========

pub async fn cleanup_old_votes(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    println!("🧹 Cleaning up old votes...");

    let collection: Collection<Vote> = state.db.collection("votes");

    let cutoff_date = Utc::now() - Duration::days(30);
    let cutoff_bson = BsonDateTime::from_chrono(cutoff_date);

    let filter = doc! {
        "voteTimestamp": {
            "$lt": cutoff_bson
        }
    };

    let delete_result = collection.delete_many(filter).await?;

    let response = json!({
        "success": true,
        "message": format!("Cleaned up {} old votes", delete_result.deleted_count),
        "deleted_count": delete_result.deleted_count,
        "timestamp": Utc::now().to_rfc3339(),
    });

    println!(
        "✅ Cleanup completed: {} votes deleted",
        delete_result.deleted_count
    );
    Ok(Json(response))
}

pub async fn get_overview_stats(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    println!("📈 Getting overview statistics...");

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let total_votes = vote_collection.estimated_document_count().await? as i64;
    let total_likes = like_collection.estimated_document_count().await? as i64;
    let total_comments = comment_collection.estimated_document_count().await? as i64;

    let home_votes = vote_collection
        .count_documents(doc! { "selection": "home_team" })
        .await? as i64;
    let draw_votes = vote_collection
        .count_documents(doc! { "selection": "draw" })
        .await? as i64;
    let away_votes = vote_collection
        .count_documents(doc! { "selection": "away_team" })
        .await? as i64;

    let home_comments = comment_collection
        .count_documents(doc! { "selection": "home_team" })
        .await? as i64;
    let draw_comments = comment_collection
        .count_documents(doc! { "selection": "draw" })
        .await? as i64;
    let away_comments = comment_collection
        .count_documents(doc! { "selection": "away_team" })
        .await? as i64;

    let today_start = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
    let today_end = Utc::now().date_naive().and_hms_opt(23, 59, 59).unwrap();

    let today_start_bson = BsonDateTime::from_chrono(today_start.and_utc());
    let today_end_bson = BsonDateTime::from_chrono(today_end.and_utc());

    let today_votes = vote_collection
        .count_documents(doc! {
            "voteTimestamp": {
                "$gte": today_start_bson,
                "$lte": today_end_bson
            }
        })
        .await? as i64;

    let stats = json!({
        "success": true,
        "data": {
            "totals": {
                "votes": total_votes,
                "likes": total_likes,
                "comments": total_comments
            },
            "vote_distribution": {
                "home_team": home_votes,
                "draw": draw_votes,
                "away_team": away_votes
            },
            "comment_distribution": {
                "home_team": home_comments,
                "draw": draw_comments,
                "away_team": away_comments
            },
            "today": {
                "votes": today_votes
            },
            "percentages": {
                "home": if total_votes > 0 { (home_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 },
                "draw": if total_votes > 0 { (draw_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 },
                "away": if total_votes > 0 { (away_votes as f64 / total_votes as f64) * 100.0 } else { 0.0 }
            }
        },
        "timestamp": Utc::now().to_rfc3339()
    });

    println!("✅ Overview stats generated");
    Ok(Json(stats))
}

// ========== ADDITIONAL HANDLERS ==========

pub async fn get_comment_counts_for_multiple_fixtures(
    State(state): State<AppState>,
    Json(fixture_ids): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "📊 Getting comment counts for {} fixtures",
        fixture_ids.len()
    );

    let collection: Collection<Comment> = state.db.collection("room");

    let mut result = serde_json::Map::new();

    for fixture_id in fixture_ids {
        let filter = doc! { "fixtureId": &fixture_id };
        let count = collection.count_documents(filter).await? as i64;
        result.insert(fixture_id, serde_json::Value::Number(count.into()));
    }

    println!("✅ Comment counts retrieved for all fixtures");
    Ok(Json(serde_json::Value::Object(result)))
}

pub async fn get_total_likes_for_multiple_fixtures(
    State(state): State<AppState>,
    Json(fixture_ids): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>> {
    println!("👍 Getting like counts for {} fixtures", fixture_ids.len());

    let collection: Collection<Like> = state.db.collection("likes");

    let mut result = serde_json::Map::new();

    for fixture_id in fixture_ids {
        let filter = doc! { "fixtureId": &fixture_id };
        let count = collection.count_documents(filter).await? as i64;
        result.insert(fixture_id, serde_json::Value::Number(count.into()));
    }

    println!("✅ Like counts retrieved for all fixtures");
    Ok(Json(serde_json::Value::Object(result)))
}

pub async fn get_combined_stats_for_multiple_fixtures(
    State(state): State<AppState>,
    Json(fixture_ids): Json<Vec<String>>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "📈 Getting combined stats for {} fixtures",
        fixture_ids.len()
    );

    let mut result = Vec::new();

    for fixture_id in fixture_ids {
        let vote_stats = get_vote_stats(State(state.clone()), Path(fixture_id.clone())).await?;
        let like_stats = get_like_stats(State(state.clone()), Path(fixture_id.clone())).await?;
        let comment_stats =
            get_comment_stats(State(state.clone()), Path(fixture_id.clone())).await?;

        let stats = json!({
            "fixture_id": fixture_id,
            "vote_stats": vote_stats.0,
            "like_stats": like_stats.0,
            "comment_stats": comment_stats.0,
        });

        result.push(stats);
    }

    println!("✅ Combined stats retrieved for all fixtures");
    Ok(Json(json!({
        "success": true,
        "data": result,
        "count": result.len(),
        "timestamp": Utc::now().to_rfc3339(),
    })))
}

// ========== REAL-TIME UPDATES HANDLERS ==========

pub async fn get_realtime_vote_updates(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "🔄 Getting real-time vote updates for fixture: {}",
        fixture_id
    );

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let home_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let like_count = like_collection
        .count_documents(doc! { "fixtureId": &fixture_id })
        .await? as i64;

    let home_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let comment_count = home_comments + draw_comments + away_comments;

    let response = json!({
        "success": true,
        "data": {
            "fixture_id": fixture_id,
            "votes": {
                "home": home_votes,
                "draw": draw_votes,
                "away": away_votes,
                "total": home_votes + draw_votes + away_votes
            },
            "likes": like_count,
            "comments": {
                "total": comment_count,
                "by_selection": {
                    "home_team": home_comments,
                    "draw": draw_comments,
                    "away_team": away_comments
                }
            },
            "total_engagement": (home_votes + draw_votes + away_votes) + like_count + comment_count,
            "last_updated": Utc::now().to_rfc3339()
        }
    });

    println!("✅ Real-time stats retrieved");
    Ok(Json(response))
}

pub async fn get_comment_counts_by_selection(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "📊 Getting comment counts by selection for fixture: {}",
        fixture_id
    );

    let collection: Collection<Comment> = state.db.collection("room");

    let home_comments = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_comments = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_comments = collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let total_comments = home_comments + draw_comments + away_comments;

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "comment_counts": {
            "home_team": home_comments,
            "draw": draw_comments,
            "away_team": away_comments,
            "total": total_comments
        },
        "percentages": {
            "home": if total_comments > 0 { (home_comments as f64 / total_comments as f64) * 100.0 } else { 0.0 },
            "draw": if total_comments > 0 { (draw_comments as f64 / total_comments as f64) * 100.0 } else { 0.0 },
            "away": if total_comments > 0 { (away_comments as f64 / total_comments as f64) * 100.0 } else { 0.0 }
        },
        "timestamp": Utc::now().to_rfc3339()
    });

    println!(
        "✅ Comment counts by selection for fixture {}: H:{} D:{} A:{}",
        fixture_id, home_comments, draw_comments, away_comments
    );
    Ok(Json(response))
}

pub async fn get_fixture_engagement_summary(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting engagement summary for fixture: {}", fixture_id);

    let vote_collection: Collection<Vote> = state.db.collection("votes");
    let like_collection: Collection<Like> = state.db.collection("likes");
    let comment_collection: Collection<Comment> = state.db.collection("room");

    let vote_filter = doc! { "fixtureId": &fixture_id };
    let total_votes = vote_collection.count_documents(vote_filter.clone()).await? as i64;
    let total_likes = like_collection.count_documents(vote_filter.clone()).await? as i64;
    let total_comments = comment_collection
        .count_documents(vote_filter.clone())
        .await? as i64;
    let total_engagement = total_votes + total_likes + total_comments;

    let home_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_votes = vote_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let home_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "home_team"
        })
        .await? as i64;

    let draw_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "draw"
        })
        .await? as i64;

    let away_comments = comment_collection
        .count_documents(doc! {
            "fixtureId": &fixture_id,
            "selection": "away_team"
        })
        .await? as i64;

    let first_vote = vote_collection.find_one(vote_filter).await?;
    let (home_team, away_team) = if let Some(vote) = first_vote {
        (vote.home_team.clone(), vote.away_team.clone())
    } else {
        ("Unknown".to_string(), "Unknown".to_string())
    };

    let engagement_score =
        (total_votes as f64 * 1.0) + (total_likes as f64 * 0.5) + (total_comments as f64 * 1.5);

    let response = json!({
        "success": true,
        "fixture_id": fixture_id,
        "home_team": home_team,
        "away_team": away_team,
        "engagement_metrics": {
            "votes": {
                "total": total_votes,
                "by_selection": {
                    "home_team": home_votes,
                    "draw": draw_votes,
                    "away_team": away_votes
                }
            },
            "likes": total_likes,
            "comments": {
                "total": total_comments,
                "by_selection": {
                    "home_team": home_comments,
                    "draw": draw_comments,
                    "away_team": away_comments
                }
            },
            "total_engagement": total_engagement,
            "engagement_score": engagement_score
        },
        "engagement_percentages": {
            "vote_percentage": if total_engagement > 0 { (total_votes as f64 / total_engagement as f64) * 100.0 } else { 0.0 },
            "like_percentage": if total_engagement > 0 { (total_likes as f64 / total_engagement as f64) * 100.0 } else { 0.0 },
            "comment_percentage": if total_engagement > 0 { (total_comments as f64 / total_engagement as f64) * 100.0 } else { 0.0 }
        },
        "timestamp": Utc::now().to_rfc3339()
    });

    println!(
        "✅ Engagement summary for fixture {}: {} total engagement",
        fixture_id, total_engagement
    );
    Ok(Json(response))
}
