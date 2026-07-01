use crate::models::user::User;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use bson::Bson;
use futures_util::StreamExt;
use mongodb::bson::{doc, DateTime};
use mongodb::Collection;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::channel::ChannelFixture;
use crate::models::game::Game;
use crate::services::fcm_service::FCMService;
use bson::oid::ObjectId;
use serde::Deserialize;

use crate::errors::{AppError, Result};
use crate::models::channel::{
    AdminRewardScore, Channel, ChannelActivity, ChannelMember, ChannelMembershipEvent, Fixture,
    Message, Payout, PendingRequest, ReplyToData, Vote, VoteCounts,
};
use crate::models::pledges::{CreatePledge, Pledge};
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

// ============================================================================
// LOG MEMBERSHIP EVENT (helper, not a route handler)
// ============================================================================

async fn log_membership_event(state: &AppState, channel_id: &str, user_id: &str, event_type: &str) {
    let events_col = state
        .db
        .collection::<ChannelMembershipEvent>("channel_membership_events");

    let event = ChannelMembershipEvent {
        id: None,
        channel_id: channel_id.to_string(),
        user_id: user_id.to_string(),
        event_type: event_type.to_string(),
        occurred_at: DateTime::now(),
    };

    if let Err(e) = events_col.insert_one(event).await {
        eprintln!("⚠️ Failed to log membership event: {}", e);
    }
}

// ============================================================================
// FINALIZE FIXTURE RESULT HANDLER
// ============================================================================

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
            "is_correct": Bson::Null
        })
        .await?;

    let mut updates = Vec::new();

    while cursor.advance().await? {
        let vote: Vote = cursor.deserialize_current()?;

        let is_correct = vote.selection == payload.result;
        let points = if is_correct {
            3
        } else if vote.selection == "draw" && payload.result != "draw" {
            -1
        } else if vote.selection != "draw" && payload.result == "draw" {
            -1
        } else {
            -3
        };

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

    // Sync updated points to all channels
    for user_id in &updates {
        let user = users_col
            .find_one(doc! { "_id": ObjectId::parse_str(user_id)? })
            .await?;

        if let Some(user) = user {
            let mut channel_cursor = channels_col
                .find(doc! { "members.user_id": user_id })
                .await?;

            while channel_cursor.advance().await? {
                let channel: Channel = channel_cursor.deserialize_current()?;

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
    let invite_code = Uuid::new_v4().to_string().to_uppercase()[0..6].to_string();

    let mut members = vec![ChannelMember {
        user_id: payload.created_by.clone(),
        username: payload.created_by_username.clone(),
        role: "admin".to_string(),
        joined_at: now,
        season_points: 0,
        correct_votes: 0,
        total_votes: 0,
        msg_count: 0,
        last_active_at: None,
    }];

    if let Some(requested_members) = payload.members {
        for new_member in requested_members {
            if new_member.user_id == payload.created_by {
                continue;
            }

            let filter = if let Ok(oid) = ObjectId::parse_str(&new_member.user_id) {
                doc! { "_id": oid }
            } else {
                doc! { "user_id": &new_member.user_id }
            };

            let user = users_col.find_one(filter).await?;

            let (season_points, correct_votes, total_votes) = if let Some(u) = user {
                (u.season_points, u.correct_votes, u.total_votes)
            } else {
                (0, 0, 0)
            };

            members.push(ChannelMember {
                user_id: new_member.user_id,
                username: new_member.username,
                role: "member".to_string(),
                joined_at: now,
                season_points,
                correct_votes,
                total_votes,
                msg_count: 0,
                last_active_at: None,
            });
        }
    }

    let member_count = members.len() as i32;

    let channel = Channel {
        id: None,
        channel_id: channel_id.clone(),
        name: payload.name,
        created_by: payload.created_by.clone(),
        created_at: now,
        members: members.clone(),
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

    channels_col.insert_one(channel).await?;

    if let Ok(oid) = ObjectId::parse_str(&payload.created_by) {
        users_col
            .update_one(doc! { "_id": oid }, doc! { "$set": { "is_admin": true } })
            .await?;
    }

    for m in &members {
        log_membership_event(&state, &channel_id, &m.user_id, "joined").await;
    }

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "invite_code": invite_code,
        "member_count": member_count,
    })))
}

// ============================================================================
// CHECK USER VOTE IN CHANNEL (deprecated - redirect to global)
// ============================================================================

pub async fn check_user_vote_in_channel_handler(
    State(state): State<AppState>,
    Path((_channel_id, fixture_id, user_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    // Deprecated - votes are now global
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
// GET SINGLE FIXTURE
// ============================================================================

pub async fn get_single_fixture_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let fixture = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "fixture": fixture,
    })))
}

// ============================================================================
// GET USER CHANNEL VOTES (deprecated - redirect to global)
// ============================================================================

pub async fn get_user_channel_votes_handler(
    State(state): State<AppState>,
    Path((_channel_id, user_id)): Path<(String, String)>,
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

    let transformed_channels: Vec<serde_json::Value> = channels
        .into_iter()
        .map(|channel: Channel| {
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

// ============================================================================
// GET MESSAGES - FIXED for general chat
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub limit: Option<i64>,
    pub before: Option<String>,
}

pub async fn get_messages_handler(
    Query(params): Query<MessagesQuery>,
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    tracing::info!(
        "📨 Fetching messages - channel: {}, fixture: {:?}",
        params.channel_id,
        params.fixture_id
    );

    let messages_col = state.db.collection::<Message>("messages");
    let limit = params.limit.unwrap_or(100);

    let mut filter = doc! {
        "channel_id": &params.channel_id,
    };

    // ✅ FIX: Use $exists:false for general chat (no fixture_id field)
    // instead of Bson::Null which only matches explicit null values
    match &params.fixture_id {
        Some(fixture_id) => {
            if fixture_id.is_empty() {
                // General chat: messages with NO fixture_id field
                filter.insert("fixture_id", doc! { "$exists": false });
            } else {
                // Fixture chat: messages with this specific fixture_id
                filter.insert("fixture_id", fixture_id);
            }
        }
        None => {
            // General chat: messages with NO fixture_id field
            filter.insert("fixture_id", doc! { "$exists": false });
        }
    }

    if let Some(before) = &params.before {
        if let Ok(before_time) = DateTime::parse_rfc3339_str(before) {
            filter.insert("sent_at", doc! { "$lt": before_time });
        }
    }

    let mut cursor = match messages_col
        .find(filter)
        .sort(doc! { "sent_at": -1 })
        .limit(limit)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("❌ Failed to query messages: {}", e);
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Database error: {}", e),
                "messages": []
            }));
        }
    };

    let mut messages: Vec<serde_json::Value> = Vec::new();
    while let Some(msg) = cursor.next().await {
        match msg {
            Ok(message) => {
                let msg_json = serde_json::json!({
                    "id": message.id.map(|oid| oid.to_hex()),
                    "message_id": message.message_id,
                    "sender_id": message.sender_id,
                    "sender_name": message.sender_name,
                    "text": message.text,
                    "selection": message.selection,
                    "sent_at": message.sent_at.to_rfc3339_string(),
                    "image_url": message.image_url,
                    "video_url": message.video_url,
                    "is_image": message.is_image,
                    "is_video": message.is_video,
                    "reply_to": message.reply_to.map(|r| serde_json::json!({
                        "messageId": r.message_id,
                        "text": r.text,
                        "username": r.username,
                        "selection": r.selection,
                        "isMe": r.is_me,
                    })),
                });
                messages.push(msg_json);
            }
            Err(e) => {
                tracing::error!("❌ Error reading message: {}", e);
            }
        }
    }

    messages.reverse();

    tracing::info!(
        "✅ Returned {} messages for channel {}, fixture: {:?}",
        messages.len(),
        params.channel_id,
        params.fixture_id
    );

    Json(serde_json::json!({
        "success": true,
        "messages": messages,
        "count": messages.len(),
        "channel_id": params.channel_id,
        "fixture_id": params.fixture_id,
    }))
}
// ============================================================================
// CAST VOTE (GLOBAL - No channel_id)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CastVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String, // "home", "away", "draw"
}

pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    use bson::DateTime as BsonDateTime;

    let games_col: Collection<Game> = state.db.collection("fixtures");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let users_col = state.db.collection::<User>("users");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::from_chrono(chrono::Utc::now());

    // Check if already voted
    let existing_voter = games_col
        .find_one(doc! {
            "match_id": &payload.fixture_id,
            "voters.userId": &payload.user_id,
        })
        .await?;

    if existing_voter.is_some() {
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    // Find user
    let user_id_obj = ObjectId::parse_str(&payload.user_id)?;
    let user = users_col
        .find_one(doc! { "_id": user_id_obj })
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    // Determine display selection
    let display_selection = match payload.selection.as_str() {
        "home" => "home_team",
        "away" => "away_team",
        "draw" => "draw",
        _ => &payload.selection,
    };

    // isCorrect / pointsAwarded start out unset — bind typed Nones first,
    // never cast inline inside doc! (the macro's token muncher chokes on
    // `as Option<T>` because of the generic angle brackets).
    let is_correct_placeholder: Option<bool> = None;
    let points_awarded_placeholder: Option<i32> = None;

    // Update fixture with vote
    games_col
        .update_one(
            doc! { "match_id": &payload.fixture_id },
            doc! {
                "$inc": { "votes": 1 },
                "$push": {
                    "voters": {
                        "userId": &payload.user_id,
                        "userName": &user.username,
                        "selection": display_selection,
                        "isCorrect": is_correct_placeholder,
                        "pointsAwarded": points_awarded_placeholder,
                        "votedAt": now,
                    }
                }
            },
        )
        .await?;

    // Update channel fixtures vote counts
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

    // Increment user total votes
    users_col
        .update_one(
            doc! { "_id": user_id_obj },
            doc! { "$inc": { "total_votes": 1 } },
        )
        .await?;

    // Update last_active_at for all channels
    let mut channel_cursor = channels_col
        .find(doc! { "members.user_id": &payload.user_id })
        .await?;

    while channel_cursor.advance().await? {
        let channel: Channel = channel_cursor.deserialize_current()?;
        channels_col
            .update_one(
                doc! {
                    "channel_id": &channel.channel_id,
                    "members.user_id": &payload.user_id,
                },
                doc! { "$set": { "members.$.last_active_at": now } },
            )
            .await?;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully",
        "fixture_id": payload.fixture_id,
        "selection": payload.selection,
    })))
}

// ============================================================================
// CREATE PLEDGE WITH ATOMIC VOTE (FIXED - CORRECT)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CreatePledgeAndVoteRequest {
    pub username: String,
    pub phone: String,
    pub selection: String,
    pub amount: f64,
    pub fan: String,
    pub home_team: String,
    pub away_team: String,
    pub starter_id: String,
    pub fixture_id: String,
}
pub async fn get_fixture_pledgers_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    // ✅ Use channel_fixtures collection, not games
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let channel_fixture = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // Pledges are stored in a separate "pledges" collection
    let pledges_col = state.db.collection::<Pledge>("pledges");
    let mut cursor = pledges_col.find(doc! { "fixture_id": &fixture_id }).await?;

    let mut pledges = Vec::new();
    while cursor.advance().await? {
        pledges.push(cursor.deserialize_current()?);
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "pledges": pledges,
        "pledgers": pledges, // pledgers = pledges (same thing)
    })))
}

pub async fn create_pledge_with_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreatePledgeAndVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "💰 Creating pledge with vote for user: {} (amount: KES {:.2})",
        payload.username, payload.amount
    );

    // ============================================================
    // 1️⃣ VALIDATE INPUT
    // ============================================================
    if payload.username.is_empty() {
        return Err(AppError::MissingRequiredField("username".to_string()));
    }
    if payload.phone.is_empty() {
        return Err(AppError::MissingRequiredField("phone".to_string()));
    }
    if payload.selection.is_empty() {
        return Err(AppError::MissingRequiredField("selection".to_string()));
    }
    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "amount must be greater than 0".to_string(),
        ));
    }
    if payload.starter_id.is_empty() {
        return Err(AppError::MissingRequiredField("starter_id".to_string()));
    }
    if payload.fixture_id.is_empty() {
        return Err(AppError::MissingRequiredField("fixture_id".to_string()));
    }

    let users_col: Collection<User> = state.db.collection("users");
    let pledges_col: Collection<Pledge> = state.db.collection("pledges");
    let games_col: Collection<Game> = state.db.collection("fixtures");
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    // ============================================================
    // 2️⃣ PARSE IDs
    // ============================================================
    let starter_id = match ObjectId::parse_str(&payload.starter_id) {
        Ok(id) => id,
        Err(_) => {
            return Err(AppError::ValidationError(
                "Invalid starter_id format".to_string(),
            ));
        }
    };

    let fixture_id = payload.fixture_id.clone();

    // ============================================================
    // 3️⃣ START SESSION & TRANSACTION
    // ============================================================
    let mut session: mongodb::ClientSession = state.client.start_session().await?;
    session.start_transaction().await?;

    // ============================================================
    // 4️⃣ FIND USER BY _id (✅ FIXED - was "user_id")
    // ============================================================
    let user = users_col
        .find_one(doc! { "_id": starter_id })
        .session(&mut session)
        .await?
        .ok_or_else(|| {
            println!("❌ User not found with _id: {}", payload.starter_id);
            AppError::DocumentNotFound
        })?;

    println!(
        "✅ Found user: {} (balance: {:.2})",
        user.username, user.balance
    );

    // ============================================================
    // 5️⃣ CHECK SUFFICIENT BALANCE
    // ============================================================
    if user.balance < payload.amount {
        session.abort_transaction().await?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have KES {:.2}, need KES {:.2}",
            user.balance, payload.amount
        )));
    }

    // ============================================================
    // 6️⃣ CHECK FIXTURE EXISTS AND NOT ALREADY VOTED
    // ============================================================
    let existing_voter = games_col
        .find_one(doc! {
            "match_id": &fixture_id,
            "voters.userId": &payload.starter_id,
        })
        .session(&mut session)
        .await?;

    if existing_voter.is_some() {
        session.abort_transaction().await?;
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    let fixture_exists = games_col
        .find_one(doc! { "match_id": &fixture_id })
        .session(&mut session)
        .await?;

    if fixture_exists.is_none() {
        session.abort_transaction().await?;
        println!("❌ Fixture not found with match_id: {}", fixture_id);
        return Err(AppError::DocumentNotFound);
    }

    // ============================================================
    // 7️⃣ CREATE TIME VALUES - ✅ FIXED
    // ============================================================
    let now = chrono::Utc::now(); // For Pledge struct (chrono)
    let now_bson = bson::DateTime::from_chrono(now); // For MongoDB storage

    // ============================================================
    // 8️⃣ DEDUCT BALANCE
    // ============================================================
    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! {
                "$inc": { "balance": -payload.amount },
                "$set": { "updated_at": now_bson }  // ✅ bson::DateTime
            },
        )
        .session(&mut session)
        .await?;

    // ============================================================
    // 9️⃣ CREATE PLEDGE RECORD - Uses chrono::DateTime<Utc>
    // ============================================================
    let display_selection = match payload.selection.as_str() {
        "home" => "home_team",
        "away" => "away_team",
        "draw" => "draw",
        _ => &payload.selection,
    };

    let pledge = Pledge {
        _id: Some(ObjectId::new()),
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        selection: display_selection.to_string(),
        amount: payload.amount,
        time: now, // ✅ chrono::DateTime<Utc>
        fan: payload.fan.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        starter_id: payload.starter_id.clone(),
        fixture_id: Some(fixture_id.clone()),
        created_at: now, // ✅ chrono::DateTime<Utc>
        updated_at: now, // ✅ chrono::DateTime<Utc>
    };

    pledges_col
        .insert_one(&pledge)
        .session(&mut session)
        .await?;

    // ============================================================
    // 🔟 UPDATE FIXTURE - ADD PLEDGE + VOTE
    // ============================================================
    let pledger_entry = doc! {
        "userId": &payload.starter_id,
        "userName": &payload.username,
        "selection": display_selection,
        "amount": payload.amount,
        "pledgedAt": now_bson,  // ✅ bson::DateTime
    };

    // Bind typed Nones outside the macro — passing `null as Option<bool>` /
    // `null as Option<i32>` directly inside doc! is what produced the
    // "expected token" / "no rules expected keyword `as`" compiler errors.
    let is_correct_placeholder: Option<bool> = None;
    let points_awarded_placeholder: Option<i32> = None;

    let voter_entry = doc! {
        "userId": &payload.starter_id,
        "userName": &payload.username,
        "selection": display_selection,
        "isCorrect": is_correct_placeholder,
        "pointsAwarded": points_awarded_placeholder,
        "votedAt": now_bson,  // ✅ bson::DateTime
    };

    let increment_field = match payload.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => {
            session.abort_transaction().await?;
            return Err(AppError::ValidationError("Invalid selection".to_string()));
        }
    };

    games_col
        .update_one(
            doc! { "match_id": &fixture_id },
            doc! {
                "$inc": {
                    "pledges": 1,
                    "votes": 1,
                },
                "$push": {
                    "pledgers": pledger_entry,
                    "voters": voter_entry,
                },
                "$set": { "updated_at": now_bson }  // ✅ bson::DateTime
            },
        )
        .session(&mut session)
        .await?;

    // ============================================================
    // 1️⃣1️⃣ UPDATE CHANNEL FIXTURE VOTE COUNTS
    // ============================================================
    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &fixture_id },
            doc! { "$inc": { increment_field: 1 } },
        )
        .session(&mut session)
        .await?;

    // ============================================================
    // 1️⃣2️⃣ INCREMENT USER TOTAL VOTES
    // ============================================================
    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "total_votes": 1 } },
        )
        .session(&mut session)
        .await?;

    // ============================================================
    // 1️⃣3️⃣ UPDATE USER LAST_ACTIVE_AT IN ALL CHANNELS
    // ============================================================
    let mut channel_cursor = channels_col
        .find(doc! { "members.user_id": &payload.starter_id })
        .session(&mut session)
        .await?;

    while channel_cursor.advance(&mut session).await? {
        let channel: Channel = channel_cursor.deserialize_current()?;
        channels_col
            .update_one(
                doc! {
                    "channel_id": &channel.channel_id,
                    "members.user_id": &payload.starter_id,
                },
                doc! { "$set": { "members.$.last_active_at": now_bson } }, // ✅ bson::DateTime
            )
            .session(&mut session)
            .await?;
    }

    // ============================================================
    // 1️⃣4️⃣ COMMIT TRANSACTION
    // ============================================================
    session.commit_transaction().await?;

    let new_balance = user.balance - payload.amount;

    println!(
        "✅ Pledge + Vote complete: {} - Amount: KES {:.2} - New balance: KES {:.2}",
        payload.username, payload.amount, new_balance
    );

    Ok(Json(json!({
        "success": true,
        "message": "Pledge and vote completed successfully",
        "pledge": {
            "username": pledge.username,
            "selection": pledge.selection,
            "amount": pledge.amount,
            "home_team": pledge.home_team,
            "away_team": pledge.away_team,
        },
        "new_balance": new_balance,
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
    pub user_nickname: Option<String>,
}

pub async fn request_join_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<RequestJoinRequest>,
) -> Result<Json<serde_json::Value>> {
    println!(
        "📥 [REQUEST-JOIN] User: {} requesting to join channel: {}",
        payload.user_id, payload.channel_id
    );

    let channels_col = state.db.collection::<Channel>("channels");
    let now = mongodb::bson::DateTime::now();

    let channel = channels_col
        .find_one(doc! { "channel_id": &payload.channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    println!("📢 [REQUEST-JOIN] Found channel: {}", channel.name);

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
        user_id: payload.user_id.clone(),
        username: payload.username.clone(),
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

    println!("✅ [REQUEST-JOIN] Added to pending requests");

    let admin_user_ids: Vec<String> = channel
        .members
        .iter()
        .filter(|m| m.role == "admin")
        .map(|m| m.user_id.clone())
        .collect();

    if !admin_user_ids.is_empty() {
        let display_name = payload.user_nickname.unwrap_or(payload.username.clone());

        let notification_data = json!({
            "type": "join_request",
            "channel_id": payload.channel_id,
            "channel_name": channel.name,
            "user_id": payload.user_id,
            "username": payload.username,
            "request_id": format!("{}_{}", payload.user_id, payload.channel_id),
            "notificationType": "join_request",
        });

        let title = "📥 Join Request";
        let body = format!("{} wants to join '{}'", display_name, channel.name);

        if let Some(fcm_service) = &state.fcm_service {
            for admin_id in &admin_user_ids {
                println!(
                    "📱 [REQUEST-JOIN] Sending notification to admin: {}",
                    admin_id
                );

                let _ = fcm_service
                    .send_to_user(
                        &state,
                        admin_id,
                        title,
                        &body,
                        notification_data.clone(),
                        "join_request",
                    )
                    .await;
            }
            println!(
                "📱 [REQUEST-JOIN] Sent notifications to {} admins",
                admin_user_ids.len()
            );
        } else {
            println!("⚠️ [REQUEST-JOIN] FCM service not available, skipping notifications");
        }
    } else {
        println!("⚠️ [REQUEST-JOIN] No admins found in channel");
    }

    println!("✅ [REQUEST-JOIN] Join request processed successfully");

    Ok(Json(json!({
        "success": true,
        "message": "Join request sent to admin",
        "pending_requests_count": channel.pending_requests.len() + 1,
    })))
}

// ============================================================================
// GET ALL CHANNELS
// ============================================================================

pub async fn get_all_channels_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let mut cursor = channels_col
        .find(doc! {})
        .sort(doc! { "member_count": -1 })
        .await?;

    let mut channels = Vec::new();
    while cursor.advance().await? {
        let channel: Channel = cursor.deserialize_current()?;
        channels.push(json!({
            "channel_id": channel.channel_id,
            "name": channel.name,
            "member_count": channel.member_count,
            "created_by": channel.created_by,
            "season": channel.season,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "channels": channels,
        "count": channels.len(),
    })))
}

// ============================================================================
// GET CHANNEL BY INVITE CODE
// ============================================================================

pub async fn get_invite_channel_handler(
    State(state): State<AppState>,
    Path(invite_code): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "invite_code": &invite_code })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "channel_id": channel.channel_id,
        "channel_name": channel.name,
        "member_count": channel.member_count,
        "created_by": channel.created_by,
    })))
}

// ============================================================================
// GET PENDING REQUESTS
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
    let now = mongodb::bson::DateTime::now();

    let channel = channels_col
        .find_one(doc! { "channel_id": &payload.channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

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
        last_active_at: None,
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

    log_membership_event(&state, &payload.channel_id, &payload.user_id, "joined").await;

    if let Some(fcm_service) = &state.fcm_service {
        let notification_data = json!({
            "type": "join_approved",
            "channel_id": payload.channel_id,
            "channel_name": channel.name,
            "action": "open_channel",
            "notificationType": "join_approved",
        });

        let title = "✅ Request Approved!";
        let body = format!("You have been added to '{}' 🎉", channel.name);

        let _ = fcm_service
            .send_to_user(
                &state,
                &payload.user_id,
                title,
                &body,
                notification_data,
                "join_approved",
            )
            .await;
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

    let channel = channels_col
        .find_one(doc! { "channel_id": &payload.channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let result = channels_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "pending_requests.user_id": &payload.user_id
            },
            doc! { "$pull": { "pending_requests": { "user_id": &payload.user_id } } },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::DocumentNotFound);
    }

    if let Some(fcm_service) = &state.fcm_service {
        let notification_data = json!({
            "type": "join_rejected",
            "channel_id": payload.channel_id,
            "channel_name": channel.name,
            "notificationType": "join_rejected",
        });

        let title = "❌ Request Declined";
        let body = format!("Your request to join '{}' was declined", channel.name);

        let _ = fcm_service
            .send_to_user(
                &state,
                &payload.user_id,
                title,
                &body,
                notification_data,
                "join_rejected",
            )
            .await;
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
        user_id: payload.user_id.clone(),
        username: payload.username,
        role: "member".to_string(),
        joined_at: now,
        season_points,
        correct_votes,
        total_votes,
        msg_count: 0,
        last_active_at: None,
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

    log_membership_event(&state, &channel.channel_id, &payload.user_id, "joined").await;

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

    // Typed None bound once and reused for every member doc below.
    // Do NOT write `null as Option<DateTime>` inline inside doc! — the
    // generic's `<`/`>` breaks the macro's parser (this was the original bug).
    let no_last_active: Option<DateTime> = None;

    let mut members_to_add = Vec::new();
    let mut added_user_ids = Vec::new();

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
            "last_active_at": no_last_active,
        });

        added_user_ids.push(member.user_id.clone());
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

    for user_id in &added_user_ids {
        log_membership_event(&state, &payload.channel_id, user_id, "joined").await;
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

    log_membership_event(&state, &payload.channel_id, &payload.user_id, "left").await;

    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// COMPUTE ADMIN PAYOUT (rate-based: votes + messages, per channel)
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ComputePayoutQuery {
    pub days: Option<i64>,
}

const KES_PER_VOTE: f64 = 10.0;
const KES_PER_MESSAGE: f64 = 1.0;
const SIGNUP_BONUS: f64 = 100.0;

async fn compute_payout_for_channel(state: &AppState, channel: &Channel) -> Result<Payout> {
    let payouts_col = state.db.collection::<Payout>("payouts");

    let last_payout = payouts_col
        .find_one(doc! { "channel_id": &channel.channel_id })
        .sort(doc! { "created_at": -1 })
        .await?;

    let current_votes: i32 = channel.members.iter().map(|m| m.total_votes).sum();
    let current_messages: i32 = channel.members.iter().map(|m| m.msg_count).sum();

    let admin_user_id = channel
        .members
        .iter()
        .find(|m| m.role == "admin")
        .map(|m| m.user_id.clone())
        .unwrap_or_else(|| channel.created_by.clone());

    let (amount, payout_type) = match &last_payout {
        None => (SIGNUP_BONUS, "signup_bonus".to_string()),
        Some(p) => {
            let prev_votes = p.votes_at_payout.unwrap_or(0);
            let prev_messages = p.messages_at_payout.unwrap_or(0);

            let new_votes = (current_votes - prev_votes).max(0) as f64;
            let new_messages = (current_messages - prev_messages).max(0) as f64;

            let amt = (new_votes * KES_PER_VOTE) + (new_messages * KES_PER_MESSAGE);
            (amt, "engagement_rate".to_string())
        }
    };

    Ok(Payout {
        id: None,
        user_id: admin_user_id,
        channel_id: channel.channel_id.clone(),
        payout_type,
        amount,
        currency: "KES".to_string(),
        week: None,
        season: channel.season.clone(),
        status: "pending".to_string(),
        created_at: DateTime::now(),
        paid_at: None,
        votes_at_payout: Some(current_votes),
        messages_at_payout: Some(current_messages),
    })
}

// ============================================================================
// SINGLE CHANNEL PAYOUT
// ============================================================================

pub async fn compute_admin_payout_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(_params): Query<ComputePayoutQuery>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let payouts_col = state.db.collection::<Payout>("payouts");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let payout = compute_payout_for_channel(&state, &channel).await?;
    payouts_col.insert_one(&payout).await?;

    Ok(Json(json!({
        "success": true,
        "payout": payout,
    })))
}

// ============================================================================
// ALL CHANNELS PAYOUTS (bulk sweep)
// ============================================================================

pub async fn compute_all_admin_payouts_handler(
    State(state): State<AppState>,
    Query(_params): Query<ComputePayoutQuery>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let payouts_col = state.db.collection::<Payout>("payouts");

    let mut cursor = channels_col.find(doc! {}).await?;

    let mut computed = Vec::new();
    let mut failed: Vec<serde_json::Value> = Vec::new();

    while cursor.advance().await? {
        let channel: Channel = match cursor.deserialize_current() {
            Ok(c) => c,
            Err(e) => {
                failed.push(json!({ "error": format!("deserialize failed: {}", e) }));
                continue;
            }
        };

        match compute_payout_for_channel(&state, &channel).await {
            Ok(payout) => {
                if let Err(e) = payouts_col.insert_one(&payout).await {
                    failed.push(json!({
                        "channel_id": channel.channel_id,
                        "error": format!("insert failed: {}", e),
                    }));
                } else {
                    computed.push(json!({
                        "channel_id": payout.channel_id,
                        "admin_user_id": payout.user_id,
                        "amount": payout.amount,
                        "payout_type": payout.payout_type,
                    }));
                }
            }
            Err(e) => {
                failed.push(json!({
                    "channel_id": channel.channel_id,
                    "error": format!("{:?}", e),
                }));
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "channels_processed": computed.len(),
        "channels_failed": failed.len(),
        "results": computed,
        "failures": failed,
    })))
}

// ============================================================================
// ADMIN REWARD SCORING
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ComputeRewardScoreQuery {
    pub days: Option<i64>,
}

async fn compute_score_for_channel(
    state: &AppState,
    channel: &Channel,
    days: i64,
) -> Result<AdminRewardScore> {
    let events_col = state
        .db
        .collection::<ChannelMembershipEvent>("channel_membership_events");

    let period_end = chrono::Utc::now();
    let period_start = period_end - chrono::Duration::days(days);
    let period_start_bson = DateTime::from_chrono(period_start);
    let period_end_bson = DateTime::from_chrono(period_end);

    let member_count = (channel.member_count.max(1)) as f64;

    let active_count = channel
        .members
        .iter()
        .filter(|m| {
            m.last_active_at
                .map_or(false, |t| t.to_chrono() >= period_start)
        })
        .count() as f64;

    let voting_count = channel
        .members
        .iter()
        .filter(|m| {
            m.total_votes > 0
                && m.last_active_at
                    .map_or(false, |t| t.to_chrono() >= period_start)
        })
        .count() as f64;

    let eligible_for_retention = channel
        .members
        .iter()
        .filter(|m| m.joined_at.to_chrono() < period_start)
        .count() as f64;

    let retained = channel
        .members
        .iter()
        .filter(|m| {
            m.joined_at.to_chrono() < period_start
                && m.last_active_at
                    .map_or(false, |t| t.to_chrono() >= period_start)
        })
        .count() as f64;

    let retention_rate = if eligible_for_retention > 0.0 {
        retained / eligible_for_retention
    } else {
        0.0
    };

    let joined_count = events_col
        .count_documents(doc! {
            "channel_id": &channel.channel_id,
            "event_type": "joined",
            "occurred_at": { "$gte": period_start_bson, "$lte": period_end_bson },
        })
        .await? as i32;

    let left_count = events_col
        .count_documents(doc! {
            "channel_id": &channel.channel_id,
            "event_type": { "$in": ["left", "removed"] },
            "occurred_at": { "$gte": period_start_bson, "$lte": period_end_bson },
        })
        .await? as i32;

    let net_member_growth = joined_count - left_count;

    let active_member_ratio = active_count / member_count;
    let vote_participation = voting_count / member_count;
    let growth_term = (net_member_growth as f64 / member_count).clamp(-1.0, 1.0);

    let score = (active_member_ratio * 0.35)
        + (vote_participation * 0.30)
        + (retention_rate * 0.25)
        + (growth_term * 0.10);

    let admin_user_id = channel
        .members
        .iter()
        .find(|m| m.role == "admin")
        .map(|m| m.user_id.clone())
        .unwrap_or_else(|| channel.created_by.clone());

    Ok(AdminRewardScore {
        id: None,
        channel_id: channel.channel_id.clone(),
        admin_user_id,
        period_start: period_start_bson,
        period_end: period_end_bson,
        active_member_ratio,
        vote_participation,
        retention_rate,
        net_member_growth,
        score,
        computed_at: DateTime::now(),
    })
}

// ============================================================================
// SINGLE CHANNEL REWARD SCORE
// ============================================================================

pub async fn compute_admin_reward_score_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<ComputeRewardScoreQuery>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let scores_col = state
        .db
        .collection::<AdminRewardScore>("admin_reward_scores");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let days = params.days.unwrap_or(7);
    let reward_score = compute_score_for_channel(&state, &channel, days).await?;

    scores_col.insert_one(&reward_score).await?;

    Ok(Json(json!({
        "success": true,
        "score": reward_score,
    })))
}

// ============================================================================
// ALL CHANNELS REWARD SCORES (bulk sweep)
// ============================================================================

pub async fn compute_all_admin_reward_scores_handler(
    State(state): State<AppState>,
    Query(params): Query<ComputeRewardScoreQuery>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let scores_col = state
        .db
        .collection::<AdminRewardScore>("admin_reward_scores");

    let days = params.days.unwrap_or(7);

    let mut cursor = channels_col.find(doc! {}).await?;

    let mut computed = Vec::new();
    let mut failed: Vec<serde_json::Value> = Vec::new();

    while cursor.advance().await? {
        let channel: Channel = match cursor.deserialize_current() {
            Ok(c) => c,
            Err(e) => {
                failed.push(json!({ "error": format!("deserialize failed: {}", e) }));
                continue;
            }
        };

        match compute_score_for_channel(&state, &channel, days).await {
            Ok(reward_score) => {
                if let Err(e) = scores_col.insert_one(&reward_score).await {
                    failed.push(json!({
                        "channel_id": channel.channel_id,
                        "error": format!("insert failed: {}", e),
                    }));
                } else {
                    computed.push(json!({
                        "channel_id": reward_score.channel_id,
                        "admin_user_id": reward_score.admin_user_id,
                        "score": reward_score.score,
                    }));
                }
            }
            Err(e) => {
                failed.push(json!({
                    "channel_id": channel.channel_id,
                    "error": format!("{:?}", e),
                }));
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "channels_processed": computed.len(),
        "channels_failed": failed.len(),
        "results": computed,
        "failures": failed,
    })))
}

// ============================================================================
// ADMIN REWARD LEADERBOARD
// ============================================================================

pub async fn get_admin_reward_leaderboard_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    let scores_col = state
        .db
        .collection::<AdminRewardScore>("admin_reward_scores");

    let mut cursor = scores_col
        .find(doc! {})
        .sort(doc! { "computed_at": -1 })
        .await?;

    let mut latest_by_channel: std::collections::HashMap<String, AdminRewardScore> =
        std::collections::HashMap::new();

    while cursor.advance().await? {
        let s: AdminRewardScore = cursor.deserialize_current()?;
        latest_by_channel.entry(s.channel_id.clone()).or_insert(s);
    }

    let mut ranked: Vec<AdminRewardScore> = latest_by_channel.into_values().collect();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let leaderboard: Vec<serde_json::Value> = ranked
        .iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "rank": i + 1,
                "channel_id": s.channel_id,
                "admin_user_id": s.admin_user_id,
                "score": s.score,
                "active_member_ratio": s.active_member_ratio,
                "vote_participation": s.vote_participation,
                "retention_rate": s.retention_rate,
                "net_member_growth": s.net_member_growth,
                "period_start": s.period_start,
                "period_end": s.period_end,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "leaderboard": leaderboard,
    })))
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
// FIXTURE VOTE COUNT
// ============================================================================

pub async fn get_fixture_vote_count_handler(
    Path((channel_id, fixture_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let collection = state.db.collection::<ChannelFixture>("channel_fixtures");

    let filter = doc! {
        "channel_id": &channel_id,
        "fixture_id": &fixture_id
    };

    match collection.find_one(filter).await {
        Ok(Some(channel_fixture)) => {
            let vote_counts = channel_fixture.vote_counts;

            Json(json!({
                "success": true,
                "fixture_id": fixture_id,
                "channel_id": channel_id,
                "home_votes": vote_counts.home,
                "away_votes": vote_counts.away,
                "draw_votes": vote_counts.draw,
                "total_votes": vote_counts.home + vote_counts.away + vote_counts.draw
            }))
        }
        Ok(None) => Json(json!({
            "success": false,
            "message": "Fixture not found in this channel",
            "home_votes": 0,
            "away_votes": 0,
            "draw_votes": 0,
            "total_votes": 0
        })),
        Err(e) => Json(json!({
            "success": false,
            "message": format!("Database error: {}", e),
            "home_votes": 0,
            "away_votes": 0,
            "draw_votes": 0,
            "total_votes": 0
        })),
    }
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

    let unread_key = format!("unread_counts.{}", user_id);

    channel_fixtures_col
        .update_one(
            doc! {
                "channel_id": &channel_id,
                "fixture_id": &fixture_id,
            },
            doc! { "$set": { unread_key: 0 } },
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
