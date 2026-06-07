use crate::models::user::User;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use mongodb::bson::{doc, DateTime};
use serde_json::{json, Value};
use uuid::Uuid;

use bson::oid::ObjectId;

use crate::errors::{AppError, Result};
use crate::models::channel::{
    Channel, ChannelActivity, ChannelFixture, ChannelMember, Fixture, Message, PendingRequest,
    ReplyToData, Vote, VoteCounts,
};
use crate::AppState;

// ============================================================================
// CREATE CHANNEL
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub created_by: String,
    pub created_by_username: String,
    pub season: String,
    pub members: Option<Vec<NewMember>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NewMember {
    pub user_id: String,
    pub username: String,
}

// ============================================================================
// FINALIZE FIXTURE RESULT (Global Points + Sync to All Channels)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct FinalizeFixtureRequest {
    pub fixture_id: String,
    pub result: String, // "home", "away", "draw"
}

pub async fn finalize_fixture_result_handler(
    State(state): State<AppState>,
    Json(payload): Json<FinalizeFixtureRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    // Update fixture status
    fixtures_col
        .update_one(
            doc! { "fixture_id": &payload.fixture_id },
            doc! {
                "$set": {
                    "result": &payload.result,
                    "status": "completed",
                }
            },
        )
        .await?;

    // Update all channel fixtures
    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await?;

    // Find all votes for this fixture that haven't been processed
    let mut cursor = votes_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "is_correct": null
        })
        .await?;

    let mut updates = Vec::new();

    while cursor.advance().await? {
        let vote: Vote = cursor.deserialize_current()?;

        // Determine points based on result
        let is_correct = vote.selection == payload.result;
        let points = if is_correct {
            // Win = +3 points
            3
        } else if vote.selection == "draw" && payload.result != "draw" {
            // Draw vote but match not draw = -1
            -1
        } else if vote.selection != "draw" && payload.result == "draw" {
            // Team vote but match draw = -1
            -1
        } else {
            // Wrong team = -3
            -3
        };

        // Update vote with result
        votes_col
            .update_one(
                doc! { "_id": vote.id },
                doc! {
                    "$set": {
                        "is_correct": is_correct,
                        "points_awarded": points,
                    }
                },
            )
            .await?;

        // 1. UPDATE USER GLOBAL POINTS
        let user_result = users_col
            .update_one(
                doc! { "_id": ObjectId::parse_str(&vote.user_id)? },
                doc! {
                    "$inc": {
                        "season_points": points,
                        "correct_votes": if is_correct { 1 } else { 0 },
                    }
                },
            )
            .await?;

        if user_result.matched_count > 0 {
            updates.push(vote.user_id);
        }
    }

    // 2. SYNC UPDATED POINTS TO ALL CHANNELS
    for user_id in &updates {
        // Get current user data
        let user = users_col
            .find_one(doc! { "_id": ObjectId::parse_str(user_id)? })
            .await?;

        if let Some(user) = user {
            // Find all channels this user belongs to
            let mut channel_cursor = channels_col
                .find(doc! { "members.user_id": user_id })
                .await?;

            while channel_cursor.advance().await? {
                let channel: Channel = channel_cursor.deserialize_current()?;

                // Update the member's denormalized points in this channel
                channels_col
                    .update_one(
                        doc! {
                            "channel_id": &channel.channel_id,
                            "members.user_id": user_id,
                        },
                        doc! {
                            "$set": {
                                "members.$.season_points": user.season_points,
                                "members.$.correct_votes": user.correct_votes,
                                "members.$.total_votes": user.total_votes,
                            }
                        },
                    )
                    .await?;
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "processed": true,
        "users_updated": updates.len()
    })))
}

pub async fn create_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateChannelRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let now = DateTime::now();
    let channel_id = Uuid::new_v4().to_string();

    // Generate invite code (6 characters uppercase)
    let invite_code = Uuid::new_v4().to_string().to_uppercase()[0..6].to_string();

    // Build members list with creator as admin
    let members = vec![ChannelMember {
        user_id: payload.created_by.clone(),
        username: payload.created_by_username.clone(),
        role: "admin".to_string(),
        joined_at: now,
        season_points: 0,
        correct_votes: 0,
        total_votes: 0,
        msg_count: 0,
    }];

    let member_count = members.len() as i32;

    // Create channel document with invite_code and pending_requests
    let channel = Channel {
        id: None,
        channel_id: channel_id.clone(),
        name: payload.name,
        created_by: payload.created_by.clone(),
        created_at: now,
        members,
        activity: ChannelActivity {
            total_messages: 0,
            messages_this_week: 0,
            week_reset_at: now,
            last_message_at: None,
        },
        season: payload.season,
        member_count,
        invite_code: invite_code.clone(),
        pending_requests: vec![],
    };

    // Insert channel
    channels_col.insert_one(channel).await?;

    // Update user's is_admin field to true
    if let Ok(user_obj_id) = ObjectId::parse_str(&payload.created_by) {
        users_col
            .update_one(
                doc! { "_id": user_obj_id },
                doc! { "$set": { "is_admin": true } },
            )
            .await?;
    }

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "invite_code": invite_code,
    })))
}

// ============================================================================
// CHECK USER VOTE IN CHANNEL (for backward compatibility)
// ============================================================================

pub async fn check_user_vote_in_channel_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id, user_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    // This is deprecated - votes are now global
    // Redirect to global check
    let votes_col = state.db.collection::<Vote>("votes");

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &user_id,
        })
        .await?;

    Ok(Json(json!({
        "success": true,
        "has_voted": vote.is_some(),
        "selection": vote.as_ref().map(|v| &v.selection),
    })))
}

// ============================================================================
// GET USER CHANNEL VOTES (deprecated - redirect to global)
// ============================================================================

pub async fn get_user_channel_votes_handler(
    State(state): State<AppState>,
    Path((channel_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    // Deprecated - votes are now global, channel_id ignored
    get_user_votes_handler(State(state), Path(user_id)).await
}

// ============================================================================
// CHECK USER VOTE (GLOBAL - No channel_id)
// ============================================================================

pub async fn check_user_vote_handler(
    State(state): State<AppState>,
    Path((fixture_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &user_id,
        })
        .await?;

    Ok(Json(json!({
        "success": true,
        "has_voted": vote.is_some(),
        "selection": vote.as_ref().map(|v| &v.selection),
    })))
}

// ============================================================================
// GET USER VOTES (GLOBAL - No channel_id)
// ============================================================================

pub async fn get_user_votes_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");

    let mut cursor = votes_col
        .find(doc! {
            "user_id": &user_id,
        })
        .await?;

    let mut votes = Vec::new();
    while cursor.advance().await? {
        votes.push(cursor.deserialize_current()?);
    }

    Ok(Json(json!({
        "success": true,
        "votes": votes,
    })))
}

// ============================================================================
// GET USER CHANNELS
// ============================================================================

pub async fn get_user_channels_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let filter = doc! { "members.user_id": &user_id };
    let mut cursor = channels_col.find(filter).await?;

    let mut channels = Vec::new();
    while cursor.advance().await? {
        channels.push(cursor.deserialize_current()?);
    }

    // Transform to include is_admin
    let transformed_channels: Vec<serde_json::Value> = channels
        .into_iter()
        .map(|channel| {
            let is_admin = channel
                .members
                .iter()
                .any(|member| member.user_id == user_id && member.role == "admin");

            let mut channel_json = serde_json::to_value(channel).unwrap_or(json!({}));

            if let Some(obj) = channel_json.as_object_mut() {
                obj.insert("is_admin".to_string(), json!(is_admin));
            }

            channel_json
        })
        .collect();

    let count = transformed_channels.len();

    Ok(Json(json!({
        "success": true,
        "channels": transformed_channels,
        "count": count,
    })))
}

// ============================================================================
// GET USER CHANNEL COUNT
// ============================================================================

pub async fn get_user_channel_count_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let count = channels_col
        .count_documents(doc! { "members.user_id": &user_id })
        .await?;

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "channel_count": count,
    })))
}

// ============================================================================
// GET CHANNEL BY ID
// ============================================================================

pub async fn get_channel_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "channel": channel,
    })))
}

// ============================================================================
// GET CHANNEL LEADERBOARD
// ============================================================================

pub async fn get_channel_leaderboard_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let mut members = channel.members;
    members.sort_by(|a, b| b.season_points.cmp(&a.season_points));

    let ranked_members: Vec<serde_json::Value> = members
        .iter()
        .enumerate()
        .map(|(index, m)| {
            json!({
                "rank": index + 1,
                "user_id": m.user_id,
                "username": m.username,
                "role": m.role,
                "season_points": m.season_points,
                "correct_votes": m.correct_votes,
                "total_votes": m.total_votes,
                "accuracy": if m.total_votes > 0 {
                    (m.correct_votes as f64 / m.total_votes as f64) * 100.0
                } else {
                    0.0
                },
                "message_count": m.msg_count,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "channel_name": channel.name,
        "leaderboard": ranked_members,
        "total_members": channel.member_count,
    })))
}

// ============================================================================
// GET WEEKLY TOP CHANNEL
// ============================================================================

pub async fn get_weekly_top_channel_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let top_channel = channels_col
        .find_one(doc! {})
        .sort(doc! { "activity.messages_this_week": -1 })
        .await?;

    if let Some(channel) = top_channel {
        Ok(Json(json!({
            "success": true,
            "channel_id": channel.channel_id,
            "channel_name": channel.name,
            "admin_user_id": channel.created_by,
            "messages_this_week": channel.activity.messages_this_week,
        })))
    } else {
        Ok(Json(json!({
            "success": true,
            "channel_id": null,
        })))
    }
}

// ============================================================================
// INITIALIZE FIXTURE CHAT
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct InitializeFixtureChatRequest {
    pub channel_id: String,
    pub fixture_id: String,
}

pub async fn initialize_fixture_chat_handler(
    State(state): State<AppState>,
    Json(payload): Json<InitializeFixtureChatRequest>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();

    let existing = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": &payload.fixture_id,
        })
        .await?;

    if let Some(chat) = existing {
        return Ok(Json(json!({
            "success": true,
            "already_exists": true,
            "chat": chat,
        })));
    }

    let fixture = fixtures_col
        .find_one(doc! { "match_id": &payload.fixture_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let channel = channels_col
        .find_one(doc! { "channel_id": &payload.channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // Initialize unread_counts map with 0 for all members
    let mut unread_counts = std::collections::HashMap::new();
    for member in &channel.members {
        unread_counts.insert(member.user_id.clone(), 0);
    }

    let new_chat = ChannelFixture {
        id: None,
        channel_id: payload.channel_id,
        fixture_id: payload.fixture_id,
        match_name: format!("{} vs {}", fixture.home_team, fixture.away_team),
        kickoff_time: format!("{} {}", fixture.date_iso, fixture.time),
        status: fixture.status,
        vote_counts: VoteCounts {
            home: 0,
            away: 0,
            draw: 0,
        },
        comment_count: 0,
        unread_counts,
        last_message: None,
        last_message_at: None,
        last_sender: None,
        added_at: now,
    };

    channel_fixtures_col.insert_one(&new_chat).await?;

    Ok(Json(json!({
        "success": true,
        "already_exists": false,
        "chat": new_chat,
    })))
}

// ============================================================================
// GET CHANNEL FIXTURES
// ============================================================================

pub async fn get_channel_fixtures_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let mut cursor = channel_fixtures_col
        .find(doc! { "channel_id": &channel_id })
        .sort(doc! { "kickoff_time": -1 })
        .await?;

    let mut fixtures = Vec::new();
    while cursor.advance().await? {
        fixtures.push(cursor.deserialize_current()?);
    }

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "fixtures": fixtures,
        "count": fixtures.len(),
    })))
}

// ============================================================================
// GET MESSAGES
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct GetMessagesQuery {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

pub async fn get_messages_handler(
    State(state): State<AppState>,
    Query(params): Query<GetMessagesQuery>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let limit = params.limit.unwrap_or(50);

    let filter = if let Some(fixture_id) = &params.fixture_id {
        doc! {
            "channel_id": &params.channel_id,
            "fixture_id": fixture_id,
        }
    } else {
        doc! {
            "channel_id": &params.channel_id,
            "fixture_id": mongodb::bson::Bson::Null,
        }
    };

    let mut cursor = messages_col
        .find(filter)
        .sort(doc! { "sent_at": 1 })
        .limit(limit as i64)
        .skip(params.offset.unwrap_or(0) as u64)
        .await?;

    let mut messages = Vec::new();
    while cursor.advance().await? {
        messages.push(cursor.deserialize_current()?);
    }

    Ok(Json(json!({
        "success": true,
        "messages": messages,
        "count": messages.len(),
    })))
}

// ============================================================================
// CAST VOTE (GLOBAL - No channel_id)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CastVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String,
}

pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let now = DateTime::now();

    let existing = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await?;

    if existing.is_some() {
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    let vote = Vote {
        id: None,
        fixture_id: payload.fixture_id.clone(),
        user_id: payload.user_id.clone(),
        selection: payload.selection.clone(),
        is_correct: None,
        points_awarded: None,
        voted_at: now,
    };

    votes_col.insert_one(vote).await?;

    let increment_field = match payload.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => return Err(AppError::ValidationError("Invalid selection".to_string())),
    };

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$inc": { increment_field: 1 } },
        )
        .await?;

    let users_col = state.db.collection::<User>("users");
    let user_object_id = ObjectId::parse_str(&payload.user_id)?;

    users_col
        .update_one(
            doc! { "_id": user_object_id },
            doc! { "$inc": { "total_votes": 1 } },
        )
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully"
    })))
}

// ============================================================================
// USER REQUESTS TO JOIN CHANNEL
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct RequestJoinRequest {
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
}

pub async fn request_join_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<RequestJoinRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();

    let channel = channels_col
        .find_one(doc! { "channel_id": &payload.channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let is_member = channel.members.iter().any(|m| m.user_id == payload.user_id);
    if is_member {
        return Err(AppError::ValidationError(
            "Already a member of this channel".to_string(),
        ));
    }

    let is_pending = channel
        .pending_requests
        .iter()
        .any(|r| r.user_id == payload.user_id);
    if is_pending {
        return Err(AppError::ValidationError(
            "Join request already pending".to_string(),
        ));
    }

    let new_request = PendingRequest {
        user_id: payload.user_id,
        username: payload.username,
        requested_at: now,
    };

    let bson_request = bson::to_bson(&new_request)
        .map_err(|e| AppError::ValidationError(format!("Failed to serialize: {}", e)))?;

    channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! { "$push": { "pending_requests": bson_request } },
        )
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Join request sent to admin"
    })))
}

// ============================================================================
// GET PENDING REQUESTS FOR ADMIN
// ============================================================================

pub async fn get_pending_requests_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "pending_requests": channel.pending_requests,
        "count": channel.pending_requests.len(),
    })))
}

// ============================================================================
// ADMIN APPROVES JOIN REQUEST
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ApproveRequestRequest {
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
}

pub async fn approve_join_request_handler(
    State(state): State<AppState>,
    Json(payload): Json<ApproveRequestRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let now = DateTime::now();

    let user_obj_id = ObjectId::parse_str(&payload.user_id)?;
    let user = users_col.find_one(doc! { "_id": user_obj_id }).await?;

    let (season_points, correct_votes, total_votes) = if let Some(u) = user {
        (u.season_points, u.correct_votes, u.total_votes)
    } else {
        (0, 0, 0)
    };

    let new_member = ChannelMember {
        user_id: payload.user_id.clone(),
        username: payload.username.clone(),
        role: "member".to_string(),
        joined_at: now,
        season_points,
        correct_votes,
        total_votes,
        msg_count: 0,
    };

    let bson_member = bson::to_bson(&new_member)
        .map_err(|e| AppError::ValidationError(format!("Failed to serialize: {}", e)))?;

    let result = channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! {
                "$pull": { "pending_requests": { "user_id": &payload.user_id } },
                "$push": { "members": bson_member },
                "$inc": { "member_count": 1 }
            },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "message": "User approved and added to channel"
    })))
}

// ============================================================================
// ADMIN REJECTS JOIN REQUEST
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct RejectRequestRequest {
    pub channel_id: String,
    pub user_id: String,
}

pub async fn reject_join_request_handler(
    State(state): State<AppState>,
    Json(payload): Json<RejectRequestRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let result = channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! { "$pull": { "pending_requests": { "user_id": &payload.user_id } } },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Join request rejected"
    })))
}

// ============================================================================
// JOIN CHANNEL VIA INVITE CODE (AUTO-APPROVED)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct JoinByCodeRequest {
    pub invite_code: String,
    pub user_id: String,
    pub username: String,
}

pub async fn join_channel_by_code_handler(
    State(state): State<AppState>,
    Json(payload): Json<JoinByCodeRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let now = DateTime::now();

    let channel = channels_col
        .find_one(doc! { "invite_code": &payload.invite_code })
        .await?
        .ok_or(AppError::ValidationError("Invalid invite code".to_string()))?;

    let is_member = channel.members.iter().any(|m| m.user_id == payload.user_id);
    if is_member {
        return Err(AppError::ValidationError(
            "Already a member of this channel".to_string(),
        ));
    }

    let user_obj_id = ObjectId::parse_str(&payload.user_id)?;
    let user = users_col.find_one(doc! { "_id": user_obj_id }).await?;

    let (season_points, correct_votes, total_votes) = if let Some(u) = user {
        (u.season_points, u.correct_votes, u.total_votes)
    } else {
        (0, 0, 0)
    };

    let new_member = ChannelMember {
        user_id: payload.user_id,
        username: payload.username,
        role: "member".to_string(),
        joined_at: now,
        season_points,
        correct_votes,
        total_votes,
        msg_count: 0,
    };

    let bson_member = bson::to_bson(&new_member)
        .map_err(|e| AppError::ValidationError(format!("Failed to serialize: {}", e)))?;

    channels_col
        .update_one(
            doc! { "channel_id": &channel.channel_id },
            doc! {
                "$push": { "members": bson_member },
                "$inc": { "member_count": 1 }
            },
        )
        .await?;

    Ok(Json(json!({
        "success": true,
        "channel_id": channel.channel_id,
        "channel_name": channel.name,
        "message": "Joined channel successfully"
    })))
}

// ============================================================================
// ADD MEMBERS TO CHANNEL
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct AddMembersRequest {
    pub channel_id: String,
    pub members: Vec<NewMember>,
}

pub async fn add_members_to_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<AddMembersRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let now = DateTime::now();

    let mut members_to_add = Vec::new();

    for member in &payload.members {
        let user_obj_id = match ObjectId::parse_str(&member.user_id) {
            Ok(id) => id,
            Err(_) => continue,
        };

        let user = users_col.find_one(doc! { "_id": user_obj_id }).await?;

        let (season_points, correct_votes, total_votes) = if let Some(u) = user {
            (u.season_points, u.correct_votes, u.total_votes)
        } else {
            (0, 0, 0)
        };

        members_to_add.push(doc! {
            "user_id": &member.user_id,
            "username": &member.username,
            "role": "member",
            "joined_at": now,
            "season_points": season_points,
            "correct_votes": correct_votes,
            "total_votes": total_votes,
            "msg_count": 0,
        });
    }

    let added_count = members_to_add.len();

    let result = channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! {
                "$push": { "members": { "$each": members_to_add } },
                "$inc": { "member_count": added_count as i32 }
            },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "added_count": added_count,
    })))
}

// ============================================================================
// LEAVE CHANNEL
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct LeaveChannelRequest {
    pub channel_id: String,
    pub user_id: String,
}

pub async fn leave_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<LeaveChannelRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let result = channels_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "members.user_id": &payload.user_id,
                "members.role": { "$ne": "admin" },
            },
            doc! {
                "$pull": { "members": { "user_id": &payload.user_id } },
                "$inc": { "member_count": -1 }
            },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::ValidationError(
            "Cannot leave. Either not a member, or you are the admin".to_string(),
        ));
    }

    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// RESET WEEKLY MESSAGES
// ============================================================================

pub async fn reset_weekly_messages_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();

    channels_col
        .update_many(
            doc! {},
            doc! {
                "$set": {
                    "activity.messages_this_week": 0,
                    "activity.week_reset_at": now,
                }
            },
        )
        .await?;

    Ok(Json(
        json!({ "success": true, "message": "Weekly messages reset" }),
    ))
}

// ============================================================================
// FIXTURE COMMENT COUNT
// ============================================================================

pub async fn get_fixture_comment_count_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let result = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?;

    let count = result.map(|cf| cf.comment_count).unwrap_or(0);

    Ok(Json(json!({
        "success": true,
        "count": count,
    })))
}

// ============================================================================
// GET CHANNEL INVITE CODE
// ============================================================================

pub async fn get_channel_invite_code_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "invite_code": channel.invite_code,
    })))
}

// ============================================================================
// FIXTURE LATEST COMMENT
// ============================================================================

pub async fn get_fixture_latest_comment_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let messages_col = state.db.collection::<Message>("messages");

    let filter = doc! {
        "channel_id": &channel_id,
        "fixture_id": &fixture_id,
    };

    let mut cursor = messages_col
        .find(filter)
        .sort(doc! { "sent_at": -1 })
        .limit(1)
        .await?;

    let latest_comment = if cursor.advance().await? {
        let message: Message = cursor.deserialize_current()?;
        Some(json!({
            "id": message.message_id,
            "user_id": message.sender_id,
            "username": message.sender_name,
            "comment": message.text,
            "selection": message.selection,
            "timestamp": message.sent_at,
        }))
    } else {
        None
    };

    Ok(Json(json!({
        "success": true,
        "latest_comment": latest_comment,
    })))
}

// ============================================================================
// MARK CHAT AS READ
// ============================================================================

pub async fn mark_chat_as_read_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id, user_id)): Path<(String, String, String)>,
) -> Result<Json<Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    channel_fixtures_col
        .update_one(
            doc! {
                "channel_id": &channel_id,
                "fixture_id": &fixture_id,
            },
            doc! { "$set": { format!("unread_counts.{}", user_id): 0 } },
        )
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Chat marked as read",
    })))
}

// ============================================================================
// GET USER UNREAD COUNT
// ============================================================================

pub async fn get_user_unread_count_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id, user_id)): Path<(String, String, String)>,
) -> Result<Json<Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let result = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?;

    let unread_count = result
        .and_then(|cf| cf.unread_counts.get(&user_id).copied())
        .unwrap_or(0);

    Ok(Json(json!({
        "success": true,
        "unread_count": unread_count,
    })))
}
