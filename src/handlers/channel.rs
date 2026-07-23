use crate::models::user::User;
use axum::{
    extract::{Path, Query, State}, // ✅ REMOVED Multipart from here
    response::IntoResponse,
    Json,
};
use axum_extra::extract::Multipart; // ✅ ADD THIS - this is the correct import
use bson::{doc, oid::ObjectId, Bson, DateTime as BsonDateTime};
use futures_util::StreamExt;
use mongodb::Collection;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::actions::Bet;
use crate::models::game::Game;
use crate::services::fcm_service::FCMService;
use serde::Deserialize;

use crate::errors::{AppError, Result};
use crate::models::channel::{
    AdminRewardScore, Channel, ChannelActivity, ChannelFixture, ChannelMember,
    ChannelMembershipEvent, Fixture, Like, Message, MessageResponse, Payout, PendingRequest,
    ReplyToData, Vote, VoteCounts,
};
use crate::models::pledges::{CreatePledge, Pledge};
use crate::AppState;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn calculate_points(selection: &str, result: &str) -> i32 {
    if selection == result {
        3
    } else if selection == "draw" && result != "draw" {
        -1
    } else if selection != "draw" && result == "draw" {
        -1
    } else {
        -3
    }
}

// ============================================================================
// CHAT MEDIA UPLOAD HANDLER - ✅ FIXED
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ChatMediaUploadRequest {
    pub caption: Option<String>,
}

pub async fn upload_chat_media_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>, // ✅ Path extractor
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>> {
    let mut caption: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut thumbnail_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Multipart(format!("Failed to read multipart field: {}", e)))?
    {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "caption" => {
                caption =
                    Some(field.text().await.map_err(|e| {
                        AppError::Multipart(format!("Failed to read caption: {}", e))
                    })?);
            }
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                // ✅ Get content_type BEFORE moving field with bytes()
                mime_type = field.content_type().map(|s| s.to_string());
                file_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            AppError::Multipart(format!("Failed to read file data: {}", e))
                        })?
                        .to_vec(),
                );
            }
            "videoThumbnail" => {
                thumbnail_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            AppError::Multipart(format!("Failed to read thumbnail data: {}", e))
                        })?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let file_data = file_data.ok_or(AppError::ValidationError("No file provided".to_string()))?;
    let file_name = file_name.ok_or(AppError::ValidationError("No file name".to_string()))?;

    if user_id.is_empty() {
        return Err(AppError::ValidationError("User ID is required".to_string()));
    }

    let ext = std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let storage_service = &state.storage_service;

    let (url, public_id) =
        if ext == "mp4" || ext == "mov" || ext == "avi" || ext == "mkv" || ext == "webm" {
            storage_service
                .upload_video(&file_data, &user_id, &ext)
                .await?
        } else {
            storage_service
                .upload_image(&file_data, &user_id, &ext)
                .await?
        };

    // ✅ Upload thumbnail if provided
    let thumbnail_url = if let Some(thumb_data) = thumbnail_data {
        storage_service
            .upload_thumbnail(&thumb_data, &user_id)
            .await
            .ok()
    } else {
        None
    };

    Ok(Json(json!({
        "success": true,
        "url": url,
        "public_id": public_id,
        "file_name": file_name,
        "caption": caption,
        "mime_type": mime_type,
        "thumbnail_url": thumbnail_url,
    })))
}
// ============================================================================
// REST OF YOUR EXISTING CODE (unchanged below)
// ============================================================================

async fn find_fixture_in_both(
    fixtures_col: &Collection<Fixture>,
    games_col: &Collection<Game>,
    fixture_id: &str,
) -> Result<Option<(String, serde_json::Value)>> {
    let fixture_filter = doc! {
        "$or": [
            { "fixture_id": fixture_id },
            { "match_id": fixture_id }
        ]
    };

    if let Some(fixture) = fixtures_col.find_one(fixture_filter.clone()).await? {
        let json = serde_json::to_value(&fixture)?;
        return Ok(Some(("fixtures".to_string(), json)));
    }

    let mut game_or_clauses = vec![
        doc! { "game_id": fixture_id },
        doc! { "match_id": fixture_id },
    ];
    if let Ok(oid) = ObjectId::parse_str(fixture_id) {
        game_or_clauses.push(doc! { "_id": oid });
    }
    let game_filter = doc! { "$or": game_or_clauses };

    if let Some(game) = games_col.find_one(game_filter).await? {
        let json = serde_json::to_value(&game)?;
        return Ok(Some(("games".to_string(), json)));
    }

    Ok(None)
}

async fn update_fixture_in_both(
    fixtures_col: &Collection<Fixture>,
    games_col: &Collection<Game>,
    fixture_id: &str,
    result: &str,
) -> Result<(bool, bool)> {
    let mut updated_fixtures = false;
    let mut updated_games = false;

    let fixture_filter = doc! {
        "$or": [
            { "fixture_id": fixture_id },
            { "match_id": fixture_id }
        ]
    };

    let fixture_result = fixtures_col
        .update_one(
            fixture_filter,
            doc! {
                "$set": {
                    "result": result,
                    "status": "completed",
                }
            },
        )
        .await?;

    if fixture_result.matched_count > 0 {
        updated_fixtures = true;
    }

    let game_filter = doc! {
        "$or": [
            { "game_id": fixture_id },
            { "match_id": fixture_id }
        ]
    };

    let game_result = games_col
        .update_one(
            game_filter,
            doc! {
                "$set": {
                    "result": result,
                    "status": "completed",
                }
            },
        )
        .await?;

    if game_result.matched_count > 0 {
        updated_games = true;
    }

    Ok((updated_fixtures, updated_games))
}

async fn log_membership_event(state: &AppState, channel_id: &str, user_id: &str, event_type: &str) {
    let events_col = state
        .db
        .collection::<ChannelMembershipEvent>("channel_membership_events");

    let event = ChannelMembershipEvent {
        id: None,
        channel_id: channel_id.to_string(),
        user_id: user_id.to_string(),
        event_type: event_type.to_string(),
        occurred_at: BsonDateTime::now(),
    };

    if let Err(e) = events_col.insert_one(event).await {
        eprintln!("⚠️ Failed to log membership event: {}", e);
    }
}

// ============================================================================
// BROADCAST HELPERS
// ============================================================================

async fn broadcast_to_channel(
    state: &AppState,
    channel_id: &str,
    fixture_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) {
    let room_key = format!("{}_{}", channel_id, fixture_id);
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

async fn notify_channel_members(
    state: &AppState,
    actor_id: &str,
    channel_id: &str,
    fixture_id: &str,
    notification_type: &str,
    title: &str,
    body: &str,
    payload: serde_json::Value,
) -> Result<()> {
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or_else(|| {
            tracing::warn!("⚠️ Channel not found: {}", channel_id);
            AppError::DocumentNotFound
        })?;

    let member_ids: Vec<String> = channel
        .members
        .iter()
        .filter(|m| m.user_id != actor_id)
        .map(|m| m.user_id.clone())
        .collect();

    if member_ids.is_empty() {
        return Ok(());
    }

    broadcast_to_channel(
        state,
        channel_id,
        fixture_id,
        notification_type,
        payload.clone(),
    )
    .await;

    let fcm_service = match &state.fcm_service {
        Some(s) => s,
        None => return Ok(()),
    };

    let data = serde_json::json!({
        "type": notification_type,
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "actor_id": actor_id,
    });

    for user_id in member_ids {
        if state.is_user_online(&user_id) {
            let personal_room = format!("user_{}", user_id);
            let tx = state.get_or_create_broadcaster(&personal_room);
            let ws_message = serde_json::json!({
                "type": notification_type,
                "payload": payload.clone(),
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

pub async fn create_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateChannelRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let now = BsonDateTime::now();
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
        likes_count: 0,
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
                likes_count: 0,
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
// FINALIZE FIXTURE RESULT
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct FinalizeFixtureRequest {
    pub fixture_id: String,
    pub result: String,
}

pub async fn finalize_fixture_result_handler(
    State(state): State<AppState>,
    Json(payload): Json<FinalizeFixtureRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channels_col = state.db.collection::<Channel>("channels");
    let users_col = state.db.collection::<User>("users");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let (fixtures_updated, games_updated) = update_fixture_in_both(
        &fixtures_col,
        &games_col,
        &payload.fixture_id,
        &payload.result,
    )
    .await?;

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await?;

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
        let points = calculate_points(&vote.selection, &payload.result);

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

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR FIXTURE SETTLEMENT
    // ============================================================
    let settlement_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "result": payload.result,
        "users_updated": updates.len(),
    });

    // Get all channels with this fixture
    let mut channel_cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.fixture_id })
        .await?;

    while let Some(cf) = channel_cursor.next().await {
        let cf: ChannelFixture = cf?;
        let _ = notify_channel_members(
            &state,
            "system",
            &cf.channel_id,
            &payload.fixture_id,
            "fixture.settled",
            "⚖️ Match Settled",
            &format!("Match result: {}", payload.result),
            settlement_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "processed": true,
        "users_updated": updates.len(),
        "fixtures_updated": fixtures_updated,
        "games_updated": games_updated,
        "source": if fixtures_updated && games_updated {
            "both"
        } else if fixtures_updated {
            "fixtures"
        } else if games_updated {
            "games"
        } else {
            "none"
        }
    })))
}

// ============================================================================
// CAST VOTE
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CastVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
    pub username: String,
    pub selection: String,
    pub channel_id: Option<String>,
}

pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");
    let users_col = state.db.collection::<User>("users");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    let existing_vote = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await?;

    if existing_vote.is_some() {
        return Err(AppError::ValidationError(
            "Already voted on this fixture".to_string(),
        ));
    }

    let user_id_obj = ObjectId::parse_str(&payload.user_id)?;
    let user = users_col
        .find_one(doc! { "_id": user_id_obj })
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    let display_selection = match payload.selection.as_str() {
        "home" => "home_team",
        "away" => "away_team",
        "draw" => "draw",
        _ => &payload.selection,
    };

    let vote = Vote {
        id: None,
        fixture_id: payload.fixture_id.clone(),
        user_id: payload.user_id.clone(),
        user_name: payload.username.clone(),
        selection: display_selection.to_string(),
        is_correct: None,
        points_awarded: None,
        voted_at: now,
    };

    votes_col.insert_one(&vote).await?;

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

    let fixture_filter = doc! {
        "$or": [
            { "fixture_id": &payload.fixture_id },
            { "match_id": &payload.fixture_id }
        ]
    };

    fixtures_col
        .update_one(fixture_filter.clone(), doc! { "$inc": { "votes": 1 } })
        .await?;

    let game_filter = doc! {
        "$or": [
            { "game_id": &payload.fixture_id },
            { "match_id": &payload.fixture_id }
        ]
    };

    games_col
        .update_one(game_filter, doc! { "$inc": { "votes": 1 } })
        .await?;

    users_col
        .update_one(
            doc! { "_id": user_id_obj },
            doc! { "$inc": { "total_votes": 1 } },
        )
        .await?;

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

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR VOTE
    // ============================================================
    let vote_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "user_id": payload.user_id,
        "username": payload.username,
        "selection": payload.selection,
    });

    // If channel_id provided, notify that channel
    if let Some(channel_id) = &payload.channel_id {
        let _ = notify_channel_members(
            &state,
            &payload.user_id,
            channel_id,
            &payload.fixture_id,
            "vote.cast",
            "🗳️ New Vote",
            &format!("{} voted on the match", payload.username),
            vote_payload.clone(),
        )
        .await;
    } else {
        // Otherwise notify all channels the user is in
        let mut channel_cursor2 = channels_col
            .find(doc! { "members.user_id": &payload.user_id })
            .await?;

        while let Some(channel) = channel_cursor2.next().await {
            let channel: Channel = channel?;
            let _ = notify_channel_members(
                &state,
                &payload.user_id,
                &channel.channel_id,
                &payload.fixture_id,
                "vote.cast",
                "🗳️ New Vote",
                &format!("{} voted on the match", payload.username),
                vote_payload.clone(),
            )
            .await;
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": "Vote cast successfully",
        "fixture_id": payload.fixture_id,
        "selection": payload.selection,
    })))
}

// ============================================================================
// CHECK USER VOTE
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

pub async fn check_user_vote_in_channel_handler(
    State(state): State<AppState>,
    Path((_channel_id, fixture_id, user_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    check_user_vote_handler(State(state), Path((fixture_id, user_id))).await
}

// ============================================================================
// GET USER VOTES
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

pub async fn get_user_channel_votes_handler(
    State(state): State<AppState>,
    Path((_channel_id, user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    get_user_votes_handler(State(state), Path(user_id)).await
}

// ============================================================================
// GET VOTE COUNT
// ============================================================================

pub async fn get_vote_count_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");

    let count = votes_col
        .count_documents(doc! { "fixture_id": &fixture_id })
        .await?;

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "vote_count": count,
    })))
}

// ============================================================================
// GET CHANNEL VOTE COUNT
// ============================================================================

pub async fn get_channel_vote_count_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let result = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?;

    let vote_counts = result.map(|cf| cf.vote_counts).unwrap_or(VoteCounts {
        home: 0,
        away: 0,
        draw: 0,
    });

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "vote_counts": vote_counts,
        "total": vote_counts.home + vote_counts.away + vote_counts.draw,
    })))
}

// ============================================================================
// GET VOTE BREAKDOWN
// ============================================================================

pub async fn get_vote_breakdown_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");

    let pipeline = vec![
        doc! {
            "$match": { "fixture_id": &fixture_id }
        },
        doc! {
            "$group": {
                "_id": "$selection",
                "count": { "$sum": 1 }
            }
        },
    ];

    let mut cursor = votes_col.aggregate(pipeline).await?;
    let mut breakdown = HashMap::new();
    let mut total = 0;

    while cursor.advance().await? {
        let doc = cursor.deserialize_current()?;
        let selection: String = doc.get("_id").unwrap().as_str().unwrap().to_string();
        let count: i32 = doc.get("count").unwrap().as_i32().unwrap();
        breakdown.insert(selection, count);
        total += count;
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "breakdown": breakdown,
        "total": total,
    })))
}

// ============================================================================
// GET FIXTURE VOTERS
// ============================================================================

pub async fn get_fixture_voters_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");

    let mut cursor = votes_col.find(doc! { "fixture_id": &fixture_id }).await?;

    let mut voters = Vec::new();
    while cursor.advance().await? {
        let vote: Vote = cursor.deserialize_current()?;
        voters.push(json!({
            "user_id": vote.user_id,
            "username": vote.user_name,
            "selection": vote.selection,
            "voted_at": vote.voted_at,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "voters": voters,
        "count": voters.len(),
    })))
}

// ============================================================================
// ROLLBACK VOTE
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct RollbackVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
}

pub async fn rollback_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<RollbackVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");
    let users_col = state.db.collection::<User>("users");

    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    votes_col
        .delete_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await?;

    let decrement_field = match vote.selection.as_str() {
        "home_team" | "home" => "vote_counts.home",
        "away_team" | "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => return Err(AppError::ValidationError("Invalid selection".to_string())),
    };

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$inc": { decrement_field: -1 } },
        )
        .await?;

    let fixture_filter = doc! {
        "$or": [
            { "fixture_id": &payload.fixture_id },
            { "match_id": &payload.fixture_id }
        ]
    };

    fixtures_col
        .update_one(fixture_filter.clone(), doc! { "$inc": { "votes": -1 } })
        .await?;

    let game_filter = doc! {
        "$or": [
            { "game_id": &payload.fixture_id },
            { "match_id": &payload.fixture_id }
        ]
    };

    games_col
        .update_one(game_filter, doc! { "$inc": { "votes": -1 } })
        .await?;

    users_col
        .update_one(
            doc! { "_id": ObjectId::parse_str(&payload.user_id)? },
            doc! { "$inc": { "total_votes": -1 } },
        )
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Vote rolled back successfully",
        "fixture_id": payload.fixture_id,
        "user_id": payload.user_id,
    })))
}

// ============================================================================
// BET HANDLERS
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct CreateBetRequest {
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String,
    pub amount: f64,
    pub fixture_id: String,
    pub vote_id: String,
    pub channel_id: Option<String>,
}

pub async fn create_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateBetRequest>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");
    let users_col = state.db.collection::<User>("users");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    let user_id_obj = ObjectId::parse_str(&payload.starter_id)?;
    let user = users_col
        .find_one(doc! { "_id": user_id_obj })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    if user.balance < payload.amount {
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have KES {:.2}, need KES {:.2}",
            user.balance, payload.amount
        )));
    }

    let votes_col = state.db.collection::<Vote>("votes");
    let vote = votes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.starter_id,
        })
        .await?
        .ok_or(AppError::ValidationError(
            "Vote not found for this fixture".to_string(),
        ))?;

    let bet = Bet::new_open(
        payload.fixture_id.clone(),
        payload.starter_id.clone(),
        payload.starter_name.clone(),
        payload.starter_selection.clone(),
        payload.amount,
        vote.id.unwrap().to_hex(),
    );

    let inserted = bets_col.insert_one(&bet).await?;
    let bet_id = inserted.inserted_id.as_object_id().unwrap();

    users_col
        .update_one(
            doc! { "_id": user_id_obj },
            doc! {
                "$inc": { "balance": -payload.amount },
                "$set": { "updated_at": now }
            },
        )
        .await?;

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$inc": { "bet_count": 1 } },
        )
        .await?;

    let created_bet = bets_col
        .find_one(doc! { "_id": bet_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR PLEDGE
    // ============================================================
    let pledge_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "user_id": payload.starter_id,
        "username": payload.starter_name,
        "selection": payload.starter_selection,
        "amount": payload.amount,
        "bet_id": bet_id.to_hex(),
    });

    if let Some(channel_id) = &payload.channel_id {
        let _ = notify_channel_members(
            &state,
            &payload.starter_id,
            channel_id,
            &payload.fixture_id,
            "pledge.create",
            "💰 New Pledge",
            &format!("{} pledged KES {:.2}", payload.starter_name, payload.amount),
            pledge_payload.clone(),
        )
        .await;
    } else {
        let mut channel_cursor = channels_col
            .find(doc! { "members.user_id": &payload.starter_id })
            .await?;

        while let Some(channel) = channel_cursor.next().await {
            let channel: Channel = channel?;
            let _ = notify_channel_members(
                &state,
                &payload.starter_id,
                &channel.channel_id,
                &payload.fixture_id,
                "pledge.create",
                "💰 New Pledge",
                &format!("{} pledged KES {:.2}", payload.starter_name, payload.amount),
                pledge_payload.clone(),
            )
            .await;
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": "Bet placed successfully",
        "bet": {
            "id": created_bet.id.unwrap().to_hex(),
            "fixture_id": created_bet.fixture_id,
            "starter_id": created_bet.starter_id,
            "starter_name": created_bet.starter_name,
            "starter_selection": created_bet.starter_selection,
            "starter_amount": created_bet.starter_amount,
            "vote_id": created_bet.vote_id,
            "status": created_bet.status,
            "created_at": created_bet.created_at,
        },
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct FillBetRequest {
    pub bet_id: String,
    pub finisher_id: String,
    pub finisher_name: String,
    pub finisher_selection: String,
    pub amount: f64,
    pub channel_id: Option<String>,
}

pub async fn fill_bet_handler(
    State(state): State<AppState>,
    Json(payload): Json<FillBetRequest>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");
    let users_col = state.db.collection::<User>("users");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    let bet_oid = ObjectId::parse_str(&payload.bet_id)?;

    let bet = bets_col
        .find_one(doc! {
            "_id": bet_oid,
            "status": "open"
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let finisher_obj_id = ObjectId::parse_str(&payload.finisher_id)?;
    let finisher = users_col
        .find_one(doc! { "_id": finisher_obj_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    if finisher.balance < payload.amount {
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have KES {:.2}, need KES {:.2}",
            finisher.balance, payload.amount
        )));
    }

    let votes_col = state.db.collection::<Vote>("votes");
    let _finisher_vote = votes_col
        .find_one(doc! {
            "fixture_id": &bet.fixture_id,
            "user_id": &payload.finisher_id,
        })
        .await?
        .ok_or(AppError::ValidationError(
            "Finisher must vote on this fixture first".to_string(),
        ))?;

    let finisher_id = payload.finisher_id.clone();
    let finisher_name = payload.finisher_name.clone();
    let finisher_selection = payload.finisher_selection.clone();

    bets_col
        .update_one(
            doc! { "_id": bet_oid },
            doc! {
                "$set": {
                    "finisher_id": finisher_id,
                    "finisher_name": finisher_name,
                    "finisher_selection": finisher_selection,
                    "finisher_amount": payload.amount,
                    "status": "matched",
                    "matched_at": now,
                }
            },
        )
        .await?;

    users_col
        .update_one(
            doc! { "_id": finisher_obj_id },
            doc! {
                "$inc": { "balance": -payload.amount },
                "$set": { "updated_at": now }
            },
        )
        .await?;

    let updated_bet = bets_col
        .find_one(doc! { "_id": bet_oid })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR MATCHED BET
    // ============================================================
    let match_payload = serde_json::json!({
        "bet_id": payload.bet_id,
        "fixture_id": bet.fixture_id,
        "starter_id": bet.starter_id,
        "starter_name": bet.starter_name,
        "starter_selection": bet.starter_selection,
        "finisher_id": payload.finisher_id,
        "finisher_name": payload.finisher_name,
        "finisher_selection": payload.finisher_selection,
        "amount": payload.amount,
        "total_pot": bet.starter_amount + payload.amount,
    });

    if let Some(channel_id) = &payload.channel_id {
        let _ = notify_channel_members(
            &state,
            &payload.finisher_id,
            channel_id,
            &bet.fixture_id,
            "bet.matched",
            "🤝 Bet Matched",
            &format!(
                "{} matched a bet with {}",
                payload.finisher_name, bet.starter_name
            ),
            match_payload.clone(),
        )
        .await;
    } else {
        // Notify both starter and finisher's channels
        let mut all_channel_ids = Vec::new();
        let mut channel_cursor = channels_col
            .find(doc! { "members.user_id": &bet.starter_id })
            .await?;
        while let Some(channel) = channel_cursor.next().await {
            let channel: Channel = channel?;
            all_channel_ids.push(channel.channel_id);
        }

        let mut channel_cursor2 = channels_col
            .find(doc! { "members.user_id": &payload.finisher_id })
            .await?;
        while let Some(channel) = channel_cursor2.next().await {
            let channel: Channel = channel?;
            if !all_channel_ids.contains(&channel.channel_id) {
                all_channel_ids.push(channel.channel_id);
            }
        }

        for channel_id in all_channel_ids {
            let _ = notify_channel_members(
                &state,
                &payload.finisher_id,
                &channel_id,
                &bet.fixture_id,
                "bet.matched",
                "🤝 Bet Matched",
                &format!(
                    "{} matched a bet with {}",
                    payload.finisher_name, bet.starter_name
                ),
                match_payload.clone(),
            )
            .await;
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": "Bet filled successfully",
        "bet": {
            "id": updated_bet.id.unwrap().to_hex(),
            "fixture_id": updated_bet.fixture_id,
            "starter_id": updated_bet.starter_id,
            "starter_name": updated_bet.starter_name,
            "starter_amount": updated_bet.starter_amount,
            "starter_selection": updated_bet.starter_selection,
            "finisher_id": updated_bet.finisher_id,
            "finisher_name": updated_bet.finisher_name,
            "finisher_selection": updated_bet.finisher_selection,
            "finisher_amount": updated_bet.finisher_amount,
            "vote_id": updated_bet.vote_id,
            "status": updated_bet.status,
            "total_pot": updated_bet.total_pot(),
        },
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SettleBetRequest {
    pub fixture_id: String,
    pub result: String,
}

pub async fn settle_bets_handler(
    State(state): State<AppState>,
    Json(payload): Json<SettleBetRequest>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");
    let users_col = state.db.collection::<User>("users");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let now = BsonDateTime::now();

    // Get all channels for this fixture
    let mut channel_cursor = channel_fixtures_col
        .find(doc! { "fixture_id": &payload.fixture_id })
        .await?;

    let mut all_channel_ids = Vec::new();
    while let Some(cf) = channel_cursor.next().await {
        let cf: ChannelFixture = cf?;
        all_channel_ids.push(cf.channel_id);
    }

    let mut cursor = bets_col
        .find(doc! {
            "fixture_id": &payload.fixture_id,
            "status": "matched"
        })
        .await?;

    let mut settled_count = 0;
    let mut results = Vec::new();

    while cursor.advance().await? {
        let bet: Bet = cursor.deserialize_current()?;

        let starter_wins = bet.starter_selection == payload.result;
        let finisher_wins = bet
            .finisher_selection
            .as_ref()
            .map(|s| s == &payload.result)
            .unwrap_or(false);

        let (winner_id, starter_result, finisher_result) = if starter_wins && finisher_wins {
            (None, Some("draw".to_string()), Some("draw".to_string()))
        } else if starter_wins {
            (
                Some(bet.starter_id.clone()),
                Some("won".to_string()),
                Some("lost".to_string()),
            )
        } else if finisher_wins {
            (
                bet.finisher_id.clone(),
                Some("lost".to_string()),
                Some("won".to_string()),
            )
        } else {
            (None, Some("lost".to_string()), Some("lost".to_string()))
        };

        bets_col
            .update_one(
                doc! { "_id": bet.id },
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
            .await?;

        let total_pot = bet.total_pot();

        if let Some(winner) = winner_id {
            let winner_obj_id = ObjectId::parse_str(&winner)?;
            users_col
                .update_one(
                    doc! { "_id": winner_obj_id },
                    doc! {
                        "$inc": { "balance": total_pot },
                        "$set": { "updated_at": now }
                    },
                )
                .await?;

            results.push(json!({
                "bet_id": bet.id.unwrap().to_hex(),
                "winner": winner,
                "payout": total_pot,
            }));
        } else if starter_result == Some("draw".to_string())
            && finisher_result == Some("draw".to_string())
        {
            let starter_obj_id = ObjectId::parse_str(&bet.starter_id)?;
            users_col
                .update_one(
                    doc! { "_id": starter_obj_id },
                    doc! {
                        "$inc": { "balance": bet.starter_amount },
                        "$set": { "updated_at": now }
                    },
                )
                .await?;

            if let Some(finisher_id) = &bet.finisher_id {
                let finisher_obj_id = ObjectId::parse_str(finisher_id)?;
                users_col
                    .update_one(
                        doc! { "_id": finisher_obj_id },
                        doc! {
                            "$inc": { "balance": bet.finisher_amount.unwrap_or(0.0) },
                            "$set": { "updated_at": now }
                        },
                    )
                    .await?;
            }

            results.push(json!({
                "bet_id": bet.id.unwrap().to_hex(),
                "result": "draw",
                "message": "Both correct - refunded",
            }));
        } else {
            results.push(json!({
                "bet_id": bet.id.unwrap().to_hex(),
                "result": "house_wins",
                "message": "Both wrong - house keeps pot",
            }));
        }

        settled_count += 1;
    }

    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");

    update_fixture_in_both(
        &fixtures_col,
        &games_col,
        &payload.fixture_id,
        &payload.result,
    )
    .await?;

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await?;

    // ============================================================
    // ✅ SEND SETTLEMENT NOTIFICATIONS
    // ============================================================
    let settlement_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "result": payload.result,
        "settled_count": settled_count,
    });

    for channel_id in all_channel_ids {
        let _ = notify_channel_members(
            &state,
            "system",
            &channel_id,
            &payload.fixture_id,
            "bet.settled",
            "⚖️ Bet Settlement",
            &format!("Bets settled. Result: {}", payload.result),
            settlement_payload.clone(),
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "All bets settled successfully",
        "fixture_id": payload.fixture_id,
        "result": payload.result,
        "settled_count": settled_count,
        "results": results,
    })))
}

pub async fn get_user_bets_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");

    let mut cursor = bets_col
        .find(doc! {
            "$or": [
                { "starter_id": &user_id },
                { "finisher_id": &user_id },
            ]
        })
        .sort(doc! { "created_at": -1 })
        .await?;

    let mut bets = Vec::new();
    while cursor.advance().await? {
        let bet: Bet = cursor.deserialize_current()?;
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

    Ok(Json(json!({
        "success": true,
        "bets": bets,
        "count": bets.len(),
    })))
}

pub async fn get_fixture_bets_handler(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");

    let mut cursor = bets_col
        .find(doc! { "fixture_id": &fixture_id })
        .sort(doc! { "created_at": -1 })
        .await?;

    let mut bets = Vec::new();
    while cursor.advance().await? {
        let bet: Bet = cursor.deserialize_current()?;
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
            "status": bet.status,
            "total_pot": bet.total_pot(),
        }));
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "bets": bets,
        "count": bets.len(),
    })))
}

pub async fn get_bet_handler(
    State(state): State<AppState>,
    Path(bet_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let bets_col = state.db.collection::<Bet>("bets");

    let bet_oid = ObjectId::parse_str(&bet_id)?;

    let bet = bets_col
        .find_one(doc! { "_id": bet_oid })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    Ok(Json(json!({
        "success": true,
        "bet": {
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
        },
    })))
}

// ============================================================================
// GET CHANNEL MEMBERS
// ============================================================================

pub async fn get_channel_members_handler(
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
        "channel_id": channel_id,
        "members": channel.members,
        "count": channel.member_count,
    })))
}

// ============================================================================
// GET CHANNEL PLEDGES
// ============================================================================

pub async fn get_channel_pledges_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let pledges_col = state.db.collection::<Pledge>("pledges");
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let member_map: HashMap<String, String> = channel
        .members
        .iter()
        .map(|m| (m.user_id.clone(), m.username.clone()))
        .collect();

    let mut cursor = pledges_col.find(doc! { "fixture_id": &fixture_id }).await?;

    let mut pledges = Vec::new();
    while cursor.advance().await? {
        let pledge: Pledge = cursor.deserialize_current()?;
        if let Some(username) = member_map.get(&pledge.starter_id) {
            let mut pledge_json = serde_json::to_value(&pledge)?;
            if let Some(obj) = pledge_json.as_object_mut() {
                obj.insert("username".to_string(), json!(username));
            }
            pledges.push(pledge_json);
        }
    }

    let total_amount: f64 = pledges
        .iter()
        .filter_map(|p| p.get("amount").and_then(|a| a.as_f64()))
        .sum();

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "pledges": pledges,
        "count": pledges.len(),
        "total_amount": total_amount,
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
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");

    let channel_fixture = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?;

    let fixture_data = find_fixture_in_both(&fixtures_col, &games_col, &fixture_id).await?;

    if let Some((source, fixture_json)) = fixture_data {
        return Ok(Json(json!({
            "success": true,
            "channel_fixture": channel_fixture,
            "fixture": fixture_json,
            "source": source,
        })));
    }

    if let Some(cf) = channel_fixture {
        return Ok(Json(json!({
            "success": true,
            "channel_fixture": cf,
            "source": "channel_fixtures_only",
        })));
    }

    Err(AppError::DocumentNotFound)
}

// ============================================================================
// GET CHANNEL FIXTURES
// ============================================================================

pub async fn get_channel_fixtures_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let games_col = state.db.collection::<Game>("games");

    let mut cursor = channel_fixtures_col
        .find(doc! { "channel_id": &channel_id })
        .sort(doc! { "kickoff_time": -1 })
        .await?;

    let mut fixtures = Vec::new();
    while cursor.advance().await? {
        let channel_fixture: ChannelFixture = cursor.deserialize_current()?;

        let fixture_data =
            find_fixture_in_both(&fixtures_col, &games_col, &channel_fixture.fixture_id).await?;

        fixtures.push(json!({
            "channel_fixture": channel_fixture,
            "fixture_data": fixture_data.map(|(source, data)| json!({
                "source": source,
                "data": data,
            })),
        }));
    }

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
        "fixtures": fixtures,
        "count": fixtures.len(),
    })))
}

// ============================================================================
// GET CHANNEL VOTES
// ============================================================================

pub async fn get_channel_votes_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let votes_col: Collection<Vote> = state.db.collection("votes");
    let channels_col: Collection<Channel> = state.db.collection("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?
        .ok_or(AppError::DocumentNotFound)?;

    let member_map: HashMap<String, String> = channel
        .members
        .iter()
        .map(|m| (m.user_id.clone(), m.username.clone()))
        .collect();

    let mut cursor = votes_col
        .find(doc! { "fixture_id": &fixture_id })
        .await
        .map_err(|e| AppError::MongoDB(e))?;

    let mut channel_votes = Vec::new();
    let mut total_votes = 0;
    let mut home_votes = 0;
    let mut away_votes = 0;
    let mut draw_votes = 0;

    while let Some(vote_result) = cursor.next().await {
        let vote: Vote = vote_result.map_err(|e| AppError::MongoDB(e))?;

        if let Some(username) = member_map.get(&vote.user_id) {
            total_votes += 1;
            match vote.selection.as_str() {
                "home_team" | "home" => home_votes += 1,
                "away_team" | "away" => away_votes += 1,
                "draw" => draw_votes += 1,
                _ => {}
            }

            channel_votes.push(json!({
                "user_id": vote.user_id,
                "user_name": username,
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
        "votes": channel_votes,
        "count": channel_votes.len(),
        "vote_counts": {
            "home": home_votes,
            "away": away_votes,
            "draw": draw_votes,
            "total": total_votes,
        },
    })))
}

// ============================================================================
// GET FIXTURE PLEDGERS
// ============================================================================

pub async fn get_fixture_pledgers_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let pledges_col = state.db.collection::<Pledge>("pledges");
    let channels_col = state.db.collection::<Channel>("channels");

    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let member_map: HashMap<String, String> = channel
        .members
        .iter()
        .map(|m| (m.user_id.clone(), m.username.clone()))
        .collect();

    let mut cursor = pledges_col.find(doc! { "fixture_id": &fixture_id }).await?;

    let mut pledges = Vec::new();
    let mut pledgers = Vec::new();

    while cursor.advance().await? {
        let pledge: Pledge = cursor.deserialize_current()?;

        if member_map.contains_key(&pledge.starter_id) {
            let username = member_map
                .get(&pledge.starter_id)
                .cloned()
                .unwrap_or(pledge.username.clone());

            pledges.push(json!({
                "username": username,
                "phone": pledge.phone,
                "selection": pledge.selection,
                "amount": pledge.amount,
                "time": pledge.time,
                "fan": pledge.fan,
                "starter_id": pledge.starter_id,
                "created_at": pledge.created_at,
            }));

            pledgers.push(json!({
                "username": username,
                "amount": pledge.amount,
                "selection": pledge.selection,
                "starter_id": pledge.starter_id,
            }));
        }
    }

    Ok(Json(json!({
        "success": true,
        "fixture_id": fixture_id,
        "channel_id": channel_id,
        "pledges": pledges,
        "pledgers": pledgers,
        "count": pledges.len(),
    })))
}

// ============================================================================
// CREATE PLEDGE WITH VOTE
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
    pub channel_id: Option<String>,
}

pub async fn create_pledge_with_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreatePledgeAndVoteRequest>,
) -> Result<Json<serde_json::Value>> {
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
    let channel_fixtures_col: Collection<ChannelFixture> = state.db.collection("channel_fixtures");
    let fixtures_col: Collection<Fixture> = state.db.collection("fixtures");
    let games_col: Collection<Game> = state.db.collection("games");
    let channels_col: Collection<Channel> = state.db.collection("channels");
    let votes_col: Collection<Vote> = state.db.collection("votes");

    let starter_id = match ObjectId::parse_str(&payload.starter_id) {
        Ok(id) => id,
        Err(_) => {
            return Err(AppError::ValidationError(
                "Invalid starter_id format".to_string(),
            ));
        }
    };

    let fixture_id = payload.fixture_id.clone();
    let channel_id = payload.channel_id.clone().unwrap_or_default();

    let mut session: mongodb::ClientSession = state.client.start_session().await?;
    session.start_transaction().await?;

    let user = users_col
        .find_one(doc! { "_id": starter_id })
        .session(&mut session)
        .await?
        .ok_or_else(|| AppError::DocumentNotFound)?;

    if user.balance < payload.amount {
        session.abort_transaction().await?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have KES {:.2}, need KES {:.2}",
            user.balance, payload.amount
        )));
    }

    let existing_vote = votes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "user_id": &payload.starter_id,
        })
        .session(&mut session)
        .await?;

    let display_selection = match payload.selection.as_str() {
        "home" => "home_team",
        "away" => "away_team",
        "draw" => "draw",
        _ => {
            session.abort_transaction().await?;
            return Err(AppError::ValidationError("Invalid selection".to_string()));
        }
    };

    let now = chrono::Utc::now();
    let now_bson = BsonDateTime::from_chrono(now);

    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! {
                "$inc": { "balance": -payload.amount },
                "$set": { "updated_at": now_bson }
            },
        )
        .session(&mut session)
        .await?;

    let pledge = Pledge {
        _id: Some(ObjectId::new()),
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        selection: display_selection.to_string(),
        amount: payload.amount,
        time: now,
        fan: payload.fan.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        starter_id: payload.starter_id.clone(),
        fixture_id: Some(fixture_id.clone()),
        created_at: now,
        updated_at: now,
    };

    pledges_col
        .insert_one(&pledge)
        .session(&mut session)
        .await?;

    if existing_vote.is_none() {
        let vote = Vote {
            id: None,
            fixture_id: fixture_id.clone(),
            user_id: payload.starter_id.clone(),
            user_name: payload.username.clone(),
            selection: display_selection.to_string(),
            is_correct: None,
            points_awarded: None,
            voted_at: now_bson,
        };

        votes_col.insert_one(&vote).session(&mut session).await?;

        let increment_field = match payload.selection.as_str() {
            "home" => "vote_counts.home",
            "away" => "vote_counts.away",
            "draw" => "vote_counts.draw",
            _ => {
                session.abort_transaction().await?;
                return Err(AppError::ValidationError("Invalid selection".to_string()));
            }
        };

        channel_fixtures_col
            .update_many(
                doc! { "fixture_id": &fixture_id },
                doc! { "$inc": { increment_field: 1 } },
            )
            .session(&mut session)
            .await?;

        let fixture_filter = doc! {
            "$or": [
                { "fixture_id": &fixture_id },
                { "match_id": &fixture_id }
            ]
        };

        fixtures_col
            .update_one(fixture_filter.clone(), doc! { "$inc": { "votes": 1 } })
            .session(&mut session)
            .await?;

        let game_filter = doc! {
            "$or": [
                { "game_id": &fixture_id },
                { "match_id": &fixture_id }
            ]
        };

        games_col
            .update_one(game_filter, doc! { "$inc": { "votes": 1 } })
            .session(&mut session)
            .await?;
    }

    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &fixture_id },
            doc! { "$inc": { "pledge_count": 1 } },
        )
        .session(&mut session)
        .await?;

    users_col
        .update_one(
            doc! { "_id": starter_id },
            doc! { "$inc": { "total_votes": 1 } },
        )
        .session(&mut session)
        .await?;

    if !channel_id.is_empty() {
        channels_col
            .update_one(
                doc! {
                    "channel_id": &channel_id,
                    "members.user_id": &payload.starter_id,
                },
                doc! { "$set": { "members.$.last_active_at": now_bson } },
            )
            .session(&mut session)
            .await?;
    }

    session.commit_transaction().await?;

    let new_balance = user.balance - payload.amount;

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR PLEDGE
    // ============================================================
    let pledge_payload = serde_json::json!({
        "fixture_id": fixture_id,
        "user_id": payload.starter_id,
        "username": payload.username,
        "selection": payload.selection,
        "amount": payload.amount,
    });

    if let Some(channel_id) = &payload.channel_id {
        let _ = notify_channel_members(
            &state,
            &payload.starter_id,
            channel_id,
            &fixture_id,
            "pledge.create",
            "💰 New Pledge",
            &format!("{} pledged KES {:.2}", payload.username, payload.amount),
            pledge_payload.clone(),
        )
        .await;
    }

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
    let games_col = state.db.collection::<Game>("games");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    let existing = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": &payload.fixture_id,
        })
        .await?;

    if let Some(chat) = existing {
        let fixture_info =
            find_fixture_in_both(&fixtures_col, &games_col, &payload.fixture_id).await?;

        return Ok(Json(json!({
            "success": true,
            "already_exists": true,
            "chat": chat,
            "fixture_info": fixture_info.map(|(source, data)| json!({
                "source": source,
                "data": data,
            })),
        })));
    }

    let fixture_data = find_fixture_in_both(&fixtures_col, &games_col, &payload.fixture_id).await?;

    let (home_team, away_team, date_iso, time, status) = match &fixture_data {
        Some((source, json_data)) => {
            if source == "fixtures" {
                let fixture: Fixture = serde_json::from_value(json_data.clone())?;
                (
                    fixture.home_team,
                    fixture.away_team,
                    fixture.date_iso,
                    fixture.time,
                    fixture.status,
                )
            } else {
                let game: Game = serde_json::from_value(json_data.clone())?;
                (
                    game.home_team,
                    game.away_team,
                    game.date_iso,
                    game.time,
                    game.status,
                )
            }
        }
        None => {
            return Err(AppError::DocumentNotFound);
        }
    };

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
        match_name: format!("{} vs {}", home_team, away_team),
        kickoff_time: format!("{} {}", date_iso, time),
        status,
        vote_counts: VoteCounts {
            home: 0,
            away: 0,
            draw: 0,
        },
        comment_count: 0,
        pledge_count: 0,
        bet_count: 0,
        likes_count: 0,
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
        "fixture_info": fixture_data.map(|(source, data)| json!({
            "source": source,
            "data": data,
        })),
    })))
}

// ============================================================================
// GET MESSAGES
// ============================================================================
// ============================================================================
// MESSAGE HANDLERS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub user_id: String,
    pub username: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_caption: Option<String>,
    #[serde(default)]
    pub is_image: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    #[serde(default)]
    pub is_video: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_selection: Option<String>,
}

pub async fn send_message_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    // Validate channel exists
    let channel = channels_col
        .find_one(doc! { "channel_id": &channel_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // Check if user is a member
    let is_member = channel.members.iter().any(|m| m.user_id == payload.user_id);
    if !is_member {
        return Err(AppError::ValidationError(
            "User is not a member of this channel".to_string(),
        ));
    }

    let message_id = Uuid::new_v4().to_string();

    // Build reply_to data if provided
    let reply_to = if let Some(reply_id) = &payload.reply_to_id {
        Some(ReplyToData {
            message_id: reply_id.clone(),
            text: payload.reply_to_text.clone().unwrap_or_default(),
            username: payload.reply_to_username.clone().unwrap_or_default(),
            selection: payload.reply_to_selection.clone(),
            is_me: false,
            image_url: None,
            video_url: None,
            is_image: false,
            is_video: false,
        })
    } else {
        None
    };

    // Build the message
    let mut message = Message {
        id: Some(ObjectId::new()),
        channel_id: channel_id.clone(),
        fixture_id: payload.fixture_id.clone(),
        sender_id: payload.user_id.clone(),
        sender_name: payload.username.clone(),
        text: payload.text.clone(),
        caption: payload.caption.clone(),
        sent_at: now,
        message_id: Some(message_id.clone()),
        selection: payload.selection.clone(),
        image_url: payload.image_url.clone(),
        image_public_id: payload.image_public_id.clone(),
        image_caption: payload.image_caption.clone(),
        is_image: payload.is_image,
        video_url: payload.video_url.clone(),
        video_public_id: payload.video_public_id.clone(),
        video_thumbnail_url: payload.video_thumbnail_url.clone(),
        video_caption: payload.video_caption.clone(),
        video_duration: payload.video_duration,
        video_size: payload.video_size,
        is_video: payload.is_video,
        reply_to,
        reply_to_id: payload.reply_to_id.clone(),
    };

    // If this is a reply, try to fetch the original message for richer reply data
    if let Some(reply_id) = &payload.reply_to_id {
        if let Ok(Some(replied_msg)) = messages_col.find_one(doc! { "message_id": reply_id }).await
        {
            let mut reply_data = ReplyToData {
                message_id: reply_id.clone(),
                text: replied_msg.text.clone(),
                username: replied_msg.sender_name.clone(),
                selection: replied_msg.selection.clone(),
                is_me: replied_msg.sender_id == payload.user_id,
                image_url: replied_msg.image_url.clone(),
                video_url: replied_msg.video_url.clone(),
                is_image: replied_msg.is_image,
                is_video: replied_msg.is_video,
            };
            message.reply_to = Some(reply_data);
            message.reply_to_id = Some(reply_id.clone());
        }
    }

    messages_col.insert_one(&message).await?;

    // Update channel fixture counts
    if let Some(fixture_id) = &payload.fixture_id {
        let update_result = channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": &channel_id,
                    "fixture_id": fixture_id,
                },
                doc! {
                    "$inc": { "comment_count": 1 },
                    "$set": {
                        "last_message": &payload.text,
                        "last_message_at": now,
                        "last_sender": &payload.username,
                    }
                },
            )
            .await?;

        // If fixture doesn't exist, create it
        if update_result.matched_count == 0 {
            // Try to get fixture info from global fixtures
            let fixtures_col = state.db.collection::<Fixture>("fixtures");
            let games_col = state.db.collection::<Game>("games");

            let fixture_data = find_fixture_in_both(&fixtures_col, &games_col, fixture_id).await?;

            let (match_name, kickoff_time, status) = match &fixture_data {
                Some((source, json_data)) => {
                    if source == "fixtures" {
                        let fixture: Fixture = serde_json::from_value(json_data.clone())?;
                        (
                            format!("{} vs {}", fixture.home_team, fixture.away_team),
                            format!("{} {}", fixture.date_iso, fixture.time),
                            fixture.status,
                        )
                    } else {
                        let game: Game = serde_json::from_value(json_data.clone())?;
                        (
                            format!("{} vs {}", game.home_team, game.away_team),
                            format!("{} {}", game.date_iso, game.time),
                            game.status,
                        )
                    }
                }
                None => (
                    format!("Fixture {}", fixture_id),
                    "".to_string(),
                    "live".to_string(),
                ),
            };

            let mut unread_counts = std::collections::HashMap::new();
            for member in &channel.members {
                unread_counts.insert(member.user_id.clone(), 0);
            }

            // Create the channel fixture
            let new_fixture = ChannelFixture {
                id: None,
                channel_id: channel_id.clone(),
                fixture_id: fixture_id.clone(),
                match_name,
                kickoff_time,
                status,
                vote_counts: VoteCounts {
                    home: 0,
                    away: 0,
                    draw: 0,
                },
                comment_count: 1,
                pledge_count: 0,
                bet_count: 0,
                likes_count: 0,
                unread_counts,
                last_message: Some(payload.text.clone()),
                last_message_at: Some(now),
                last_sender: Some(payload.username.clone()),
                added_at: now,
            };

            channel_fixtures_col.insert_one(&new_fixture).await?;
        }
    }

    // Increment member message count
    channels_col
        .update_one(
            doc! {
                "channel_id": &channel_id,
                "members.user_id": &payload.user_id,
            },
            doc! {
                "$inc": { "members.$.msg_count": 1 },
                "$set": { "members.$.last_active_at": now }
            },
        )
        .await?;

    // Increment total messages in channel activity
    channels_col
        .update_one(
            doc! { "channel_id": &channel_id },
            doc! {
                "$inc": {
                    "activity.total_messages": 1,
                    "activity.messages_this_week": 1,
                },
                "$set": { "activity.last_message_at": now }
            },
        )
        .await?;

    // Increment unread counts for all other members
    let mut session = state.client.start_session().await?;
    session.start_transaction().await?;

    // Get current unread counts
    let channel_fixture = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": payload.fixture_id.as_deref().unwrap_or(""),
        })
        .session(&mut session)
        .await?;

    if let Some(mut cf) = channel_fixture {
        for member in &channel.members {
            if member.user_id != payload.user_id {
                let key = format!("unread_counts.{}", member.user_id);
                let current = cf.unread_counts.get(&member.user_id).copied().unwrap_or(0);
                let update_doc = doc! { "$set": { key: current + 1 } };
                channel_fixtures_col
                    .update_one(
                        doc! {
                            "_id": cf.id,
                        },
                        update_doc,
                    )
                    .session(&mut session)
                    .await?;
            }
        }
    }

    session.commit_transaction().await?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR NEW MESSAGE
    // ============================================================
    let message_payload = serde_json::json!({
        "messageId": message_id,
        "channel_id": channel_id,
        "fixture_id": payload.fixture_id,
        "sender_id": payload.user_id,
        "sender_name": payload.username,
        "text": payload.text,
        "selection": payload.selection,
        "caption": payload.caption,
        "image_url": payload.image_url,
        "video_url": payload.video_url,
        "is_image": payload.is_image,
        "is_video": payload.is_video,
        "reply_to": message.reply_to,
        "sent_at": now.to_rfc3339_string(),
    });

    let display_name = payload.username.clone();
    let preview = if payload.is_image {
        "📷 Image".to_string()
    } else if payload.is_video {
        "🎥 Video".to_string()
    } else {
        payload.text.chars().take(50).collect::<String>()
    };

    let _ = notify_channel_members(
        &state,
        &payload.user_id,
        &channel_id,
        payload.fixture_id.as_deref().unwrap_or("general"),
        "chat.message",
        &format!("💬 {} sent a message", display_name),
        &preview,
        message_payload,
    )
    .await;

    // Return the created message
    let response = MessageResponse::from(message);

    Ok(Json(json!({
        "success": true,
        "message": "Message sent successfully",
        "data": response,
    })))
}

// ============================================================================
// GET MESSAGES HANDLER
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    pub fixture_id: Option<String>,
    pub limit: Option<i64>,
    pub before: Option<String>,
}

pub async fn get_messages_handler(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let limit = params.limit.unwrap_or(100).min(500);

    let mut filter = doc! {
        "channel_id": &channel_id,
    };

    // Filter by fixture_id
    match &params.fixture_id {
        Some(fixture_id) => {
            if fixture_id.is_empty() {
                filter.insert(
                    "$or",
                    vec![
                        doc! { "fixture_id": doc! { "$exists": false } },
                        doc! { "fixture_id": Bson::Null },
                    ],
                );
            } else {
                filter.insert("fixture_id", fixture_id);
            }
        }
        None => {
            filter.insert(
                "$or",
                vec![
                    doc! { "fixture_id": doc! { "$exists": false } },
                    doc! { "fixture_id": Bson::Null },
                ],
            );
        }
    }

    // Filter by before timestamp
    if let Some(before) = &params.before {
        if let Ok(before_time) = BsonDateTime::parse_rfc3339_str(before) {
            filter.insert("sent_at", doc! { "$lt": before_time });
        }
    }

    let mut cursor = messages_col
        .find(filter)
        .sort(doc! { "sent_at": -1 })
        .limit(limit)
        .await?;

    let mut messages: Vec<MessageResponse> = Vec::new();
    while let Some(msg) = cursor.next().await {
        let msg: Message = msg?;
        messages.push(MessageResponse::from(msg));
    }

    // Reverse to get chronological order
    messages.reverse();

    Ok(Json(json!({
        "success": true,
        "messages": messages,
        "count": messages.len(),
        "channel_id": channel_id,
        "fixture_id": params.fixture_id,
    })))
}

// ============================================================================
// GET SINGLE MESSAGE
// ============================================================================

pub async fn get_single_message_handler(
    State(state): State<AppState>,
    Path((_channel_id, message_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");

    let message = messages_col
        .find_one(doc! { "message_id": &message_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    let response = MessageResponse::from(message);

    Ok(Json(json!({
        "success": true,
        "message": response,
    })))
}

// ============================================================================
// DELETE MESSAGE
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DeleteMessageRequest {
    pub user_id: String,
}

pub async fn delete_message_handler(
    State(state): State<AppState>,
    Path((_channel_id, message_id)): Path<(String, String)>,
    Json(payload): Json<DeleteMessageRequest>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    // Find the message
    let message = messages_col
        .find_one(doc! { "message_id": &message_id })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // Check if user owns the message
    if message.sender_id != payload.user_id {
        return Err(AppError::ValidationError(
            "You can only delete your own messages".to_string(),
        ));
    }

    // Delete the message
    messages_col.delete_one(doc! { "_id": message.id }).await?;

    // Decrement comment count in channel fixture
    if let Some(fixture_id) = &message.fixture_id {
        channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": &message.channel_id,
                    "fixture_id": fixture_id,
                },
                doc! { "$inc": { "comment_count": -1 } },
            )
            .await?;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Message deleted successfully",
        "message_id": message_id,
    })))
}

// ============================================================================
// GET MESSAGE THREAD (Replies to a message)
// ============================================================================

pub async fn get_message_thread_handler(
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");

    // Get the parent message
    let parent = messages_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "message_id": &message_id,
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // Find all replies to this message
    let mut cursor = messages_col
        .find(doc! {
            "channel_id": &channel_id,
            "reply_to_id": &message_id,
        })
        .sort(doc! { "sent_at": 1 })
        .await?;

    let mut replies: Vec<MessageResponse> = Vec::new();
    while let Some(msg) = cursor.next().await {
        let msg: Message = msg?;
        replies.push(MessageResponse::from(msg));
    }

    Ok(Json(json!({
        "success": true,
        "parent": MessageResponse::from(parent),
        "replies": replies,
        "reply_count": replies.len(),
    })))
}

// ============================================================================
// CHANNEL MANAGEMENT HANDLERS
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
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

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
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": "Join request sent to admin",
        "pending_requests_count": channel.pending_requests.len() + 1,
    })))
}

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
    let now = BsonDateTime::now();

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
        likes_count: 0,
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
    let now = BsonDateTime::now();

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
        likes_count: 0,
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
    let now = BsonDateTime::now();

    let no_last_active: Option<BsonDateTime> = None;

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
            "likes_count": 0,
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
// LIKES HANDLERS
// ============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ToggleLikeRequest {
    pub fixture_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub action: String,
}

pub async fn toggle_like_handler(
    State(state): State<AppState>,
    Json(payload): Json<ToggleLikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let likes_col = state.db.collection::<Like>("likes");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let channels_col = state.db.collection::<Channel>("channels");
    let now = BsonDateTime::now();

    let existing_like = likes_col
        .find_one(doc! {
            "fixture_id": &payload.fixture_id,
            "channel_id": &payload.channel_id,
            "user_id": &payload.user_id,
        })
        .await?;

    let is_liking = payload.action == "like";
    let is_unliking = payload.action == "unlike";

    if is_liking && existing_like.is_some() {
        return Err(AppError::ValidationError(
            "Already liked this fixture".to_string(),
        ));
    }

    if is_unliking && existing_like.is_none() {
        return Err(AppError::ValidationError(
            "Not liked this fixture".to_string(),
        ));
    }

    let mut session = state.client.start_session().await?;
    session.start_transaction().await?;

    let like_increment = if is_liking { 1 } else { -1 };

    channel_fixtures_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "fixture_id": &payload.fixture_id,
            },
            doc! { "$inc": { "likes_count": like_increment } },
        )
        .session(&mut session)
        .await?;

    if is_liking {
        let like = Like::new(
            payload.fixture_id.clone(),
            payload.channel_id.clone(),
            payload.user_id.clone(),
            payload.username.clone(),
        );
        likes_col.insert_one(&like).session(&mut session).await?;
    } else {
        likes_col
            .delete_one(doc! {
                "fixture_id": &payload.fixture_id,
                "channel_id": &payload.channel_id,
                "user_id": &payload.user_id,
            })
            .session(&mut session)
            .await?;
    }

    channels_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "members.user_id": &payload.user_id,
            },
            doc! { "$inc": { "members.$.likes_count": like_increment } },
        )
        .session(&mut session)
        .await?;

    channels_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "members.user_id": &payload.user_id,
            },
            doc! { "$set": { "members.$.last_active_at": now } },
        )
        .session(&mut session)
        .await?;

    session.commit_transaction().await?;

    let updated = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": &payload.fixture_id,
        })
        .await?
        .ok_or(AppError::DocumentNotFound)?;

    // ============================================================
    // ✅ SEND NOTIFICATIONS FOR LIKE
    // ============================================================
    let like_payload = serde_json::json!({
        "fixture_id": payload.fixture_id,
        "channel_id": payload.channel_id,
        "user_id": payload.user_id,
        "username": payload.username,
        "action": payload.action,
        "total_likes": updated.likes_count,
    });

    let _ = notify_channel_members(
        &state,
        &payload.user_id,
        &payload.channel_id,
        &payload.fixture_id,
        "like",
        "❤️ New Like",
        &format!("{} liked the match", payload.username),
        like_payload,
    )
    .await;

    Ok(Json(json!({
        "success": true,
        "fixture_id": payload.fixture_id,
        "channel_id": payload.channel_id,
        "total_likes": updated.likes_count,
        "user_has_liked": is_liking,
        "action": payload.action,
    })))
}

pub async fn get_fixture_likes_handler(
    State(state): State<AppState>,
    Path((channel_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let result = channel_fixtures_col
        .find_one(doc! {
            "channel_id": &channel_id,
            "fixture_id": &fixture_id,
        })
        .await?;

    let likes_count = result.map(|cf| cf.likes_count).unwrap_or(0);

    Ok(Json(json!({
        "success": true,
        "total_likes": likes_count,
    })))
}

pub async fn check_user_liked_handler(
    State(state): State<AppState>,
    Path((user_id, channel_id, fixture_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    let likes_col = state.db.collection::<Like>("likes");

    let has_liked = likes_col
        .find_one(doc! {
            "fixture_id": &fixture_id,
            "channel_id": &channel_id,
            "user_id": &user_id,
        })
        .await?
        .is_some();

    Ok(Json(json!({
        "success": true,
        "has_liked": has_liked,
    })))
}

pub async fn get_user_liked_fixtures_handler(
    State(state): State<AppState>,
    Path((user_id, channel_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let likes_col = state.db.collection::<Like>("likes");

    let mut cursor = likes_col
        .find(doc! {
            "user_id": &user_id,
            "channel_id": &channel_id,
        })
        .await?;

    let mut fixture_ids = Vec::new();
    while cursor.advance().await? {
        let like: Like = cursor.deserialize_current()?;
        fixture_ids.push(like.fixture_id);
    }

    Ok(Json(json!({
        "success": true,
        "fixture_ids": fixture_ids,
        "count": fixture_ids.len(),
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

// ============================================================================
// GET FIXTURE COMMENT COUNT
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
// GET FIXTURE LATEST COMMENT
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
// ADMIN PAYOUT
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
        created_at: BsonDateTime::now(),
        paid_at: None,
        votes_at_payout: Some(current_votes),
        messages_at_payout: Some(current_messages),
    })
}

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
    let period_start_bson = BsonDateTime::from_chrono(period_start);
    let period_end_bson = BsonDateTime::from_chrono(period_end);

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
        computed_at: BsonDateTime::now(),
    })
}

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
    let now = BsonDateTime::now();

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
