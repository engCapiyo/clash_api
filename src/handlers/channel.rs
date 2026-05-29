use axum::{
    extract::{Path, State},
    Json,
};
use mongodb::bson::{doc, DateTime};
use serde_json::json;
use uuid::Uuid;

use crate::errors::Result;
use crate::models::channel::{
    Channel, ChannelActivity, ChannelFixture, ChannelMember, Fixture, Message, Vote, VoteCounts,
};
use crate::AppState;

// ============================================================================
// CREATE CHANNEL
// ============================================================================

pub async fn create_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateChannelRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();
    let channel_id = Uuid::new_v4().to_string();

    // Build members list: admin first, then any passed-in members
    let mut members = vec![ChannelMember {
        user_id: payload.created_by.clone(),
        username: payload.created_by_username.clone(),
        role: "admin".to_string(),
        joined_at: now,
        season_points: 0,
        correct_votes: 0,
        total_votes: 0,
        msg_count: 0,
    }];

    // Add the selected comrades
    for m in payload.members.unwrap_or_default() {
        members.push(ChannelMember {
            user_id: m.user_id,
            username: m.username,
            role: "member".to_string(),
            joined_at: now,
            season_points: 0,
            correct_votes: 0,
            total_votes: 0,
            msg_count: 0,
        });
    }

    let member_count = members.len() as i32;

    let channel = Channel {
        id: None,
        channel_id: channel_id.clone(),
        name: payload.name,
        created_by: payload.created_by,
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
    };

    channels_col.insert_one(channel).await?;

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
    })))
}
pub async fn get_user_channels_handler(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");

    // Find channels where the user is a member
    let filter = doc! { "members.user_id": &user_id };
    let mut cursor = channels_col.find(filter).await?;

    let mut channels = Vec::new();
    while cursor.advance().await? {
        channels.push(cursor.deserialize_current()?);
    }

    Ok(Json(json!({
        "success": true,
        "channels": channels,
        "count": channels.len(),
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
        .ok_or_else(|| crate::errors::AppError::DocumentNotFound)?;

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
        .ok_or_else(|| crate::errors::AppError::DocumentNotFound)?;

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
// GET WEEKLY TOP CHANNEL (for admin payout)
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

pub async fn initialize_fixture_chat_handler(
    State(state): State<AppState>,
    Json(payload): Json<InitializeFixtureChatRequest>,
) -> Result<Json<serde_json::Value>> {
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
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
        .find_one(doc! { "fixture_id": &payload.fixture_id })
        .await?
        .ok_or_else(|| crate::errors::AppError::DocumentNotFound)?;

    let new_chat = ChannelFixture {
        id: None,
        channel_id: payload.channel_id,
        fixture_id: payload.fixture_id,
        match_name: format!("{} vs {}", fixture.home_team, fixture.away_team),
        kickoff_time: fixture.kickoff_time,
        status: fixture.status,
        vote_counts: VoteCounts {
            home: 0,
            away: 0,
            draw: 0,
        },
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
// GET CHANNEL FIXTURES (all fixtures a channel follows)
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
// SEND MESSAGE
// ============================================================================

pub async fn send_message_handler(
    State(state): State<AppState>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let channels_col = state.db.collection::<Channel>("channels");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let now = DateTime::now();

    let message = Message {
        id: None,
        channel_id: payload.channel_id.clone(),
        fixture_id: payload.fixture_id.clone(),
        sender_id: payload.sender_id.clone(),
        sender_name: payload.sender_name.clone(),
        text: payload.text.clone(),
        sent_at: now,
    };

    messages_col.insert_one(message).await?;

    // Update channel activity
    channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! {
                "$inc": {
                    "activity.total_messages": 1,
                    "activity.messages_this_week": 1,
                },
                "$set": { "activity.last_message_at": now }
            },
        )
        .await?;

    // Update member message count
    channels_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "members.user_id": &payload.sender_id
            },
            doc! { "$inc": { "members.$.msg_count": 1 } },
        )
        .await?;

    // Update fixture last message if applicable
    if let Some(fixture_id) = &payload.fixture_id {
        channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": &payload.channel_id,
                    "fixture_id": fixture_id,
                },
                doc! {
                    "$set": {
                        "last_message": &payload.text,
                        "last_message_at": now,
                        "last_sender": &payload.sender_name,
                    }
                },
            )
            .await?;
    }

    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// GET MESSAGES
// ============================================================================

// ============================================================================
// CAST VOTE
// ============================================================================
pub async fn get_messages_handler(
    State(state): State<AppState>,
    Json(payload): Json<GetMessagesRequest>,
) -> Result<Json<serde_json::Value>> {
    let messages_col = state.db.collection::<Message>("messages");
    let limit = payload.limit.unwrap_or(50);

    let filter = if let Some(fixture_id) = &payload.fixture_id {
        doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": fixture_id,
        }
    } else {
        doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": None::<String>,
        }
    };

    let mut cursor = messages_col
        .find(filter)
        .sort(doc! { "sent_at": -1 })
        .limit(limit as i64)
        .skip(payload.offset.unwrap_or(0) as u64)
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

pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let now = DateTime::now();

    // Check if already voted
    let existing = votes_col
        .find_one(doc! {
            "channel_id": &payload.channel_id,
            "fixture_id": &payload.fixture_id,
            "user_id": &payload.user_id,
        })
        .await?;

    if existing.is_some() {
        return Err(crate::errors::AppError::ValidationError(
            "Already voted".to_string(),
        ));
    }

    let vote = Vote {
        id: None,
        channel_id: payload.channel_id.clone(),
        fixture_id: payload.fixture_id.clone(),
        user_id: payload.user_id.clone(),
        selection: payload.selection.clone(),
        is_correct: None,
        points_awarded: None,
        voted_at: now,
    };

    votes_col.insert_one(vote).await?;

    // Increment vote count in channel_fixtures
    let increment_field = match payload.selection.as_str() {
        "home" => "vote_counts.home",
        "away" => "vote_counts.away",
        "draw" => "vote_counts.draw",
        _ => {
            return Err(crate::errors::AppError::ValidationError(
                "Invalid selection".to_string(),
            ))
        }
    };

    channel_fixtures_col
        .update_one(
            doc! {
                "channel_id": &payload.channel_id,
                "fixture_id": &payload.fixture_id,
            },
            doc! { "$inc": { increment_field: 1 } },
        )
        .await?;

    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// FINALIZE FIXTURE RESULT (award points after match ends)
// ============================================================================

pub async fn finalize_fixture_result_handler(
    State(state): State<AppState>,
    Json(payload): Json<FinalizeFixtureRequest>,
) -> Result<Json<serde_json::Value>> {
    let votes_col = state.db.collection::<Vote>("votes");
    let channels_col = state.db.collection::<Channel>("channels");
    let fixtures_col = state.db.collection::<Fixture>("fixtures");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    // Update fixture with result
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

    // Update all channel_fixtures status for this fixture
    channel_fixtures_col
        .update_many(
            doc! { "fixture_id": &payload.fixture_id },
            doc! { "$set": { "status": "completed" } },
        )
        .await?;

    // Find all votes for this fixture across all channels
    let mut cursor = votes_col
        .find(doc! { "fixture_id": &payload.fixture_id, "is_correct": null })
        .await?;

    while cursor.advance().await? {
        let vote: Vote = cursor.deserialize_current()?;
        let is_correct = vote.selection == payload.result;
        let points = if is_correct { 10 } else { 0 };

        // Update vote with correctness and points
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

        // Update member in channel
        if points > 0 {
            channels_col
                .update_one(
                    doc! {
                        "channel_id": &vote.channel_id,
                        "members.user_id": &vote.user_id,
                    },
                    doc! {
                        "$inc": {
                            "members.$.season_points": points,
                            "members.$.correct_votes": 1,
                            "members.$.total_votes": 1,
                        }
                    },
                )
                .await?;
        } else {
            channels_col
                .update_one(
                    doc! {
                        "channel_id": &vote.channel_id,
                        "members.user_id": &vote.user_id,
                    },
                    doc! { "$inc": { "members.$.total_votes": 1 } },
                )
                .await?;
        }
    }

    Ok(Json(json!({ "success": true, "processed": true })))
}

// ============================================================================
// ADD MEMBERS TO CHANNEL
// ============================================================================

pub async fn add_members_to_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<AddMembersRequest>,
) -> Result<Json<serde_json::Value>> {
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();

    let mut members_to_add = Vec::new();
    for member in &payload.members {
        // CHANGE: add & to iterate by reference
        members_to_add.push(doc! {
            "user_id": &member.user_id,
            "username": &member.username,
            "role": "member",
            "joined_at": now,
            "season_points": 0,
            "correct_votes": 0,
            "total_votes": 0,
            "msg_count": 0,
        });
    }

    let result = channels_col
        .update_one(
            doc! { "channel_id": &payload.channel_id },
            doc! {
                "$push": { "members": { "$each": members_to_add } },
                "$inc": { "member_count": payload.members.len() as i32 }
            },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(crate::errors::AppError::DocumentNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "added_count": payload.members.len(),
    })))
}
// ============================================================================
// LEAVE CHANNEL
// ============================================================================

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
        return Err(crate::errors::AppError::ValidationError(
            "Cannot leave. Either not a member, or you are the admin".to_string(),
        ));
    }

    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// RESET WEEKLY MESSAGES (cron job - call every Monday at 00:00)
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
// REQUEST TYPES
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
pub struct InitializeFixtureChatRequest {
    pub channel_id: String,
    pub fixture_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct SendMessageRequest {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct GetMessagesRequest {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CastVoteRequest {
    pub channel_id: String,
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct FinalizeFixtureRequest {
    pub fixture_id: String,
    pub result: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct AddMembersRequest {
    pub channel_id: String,
    pub members: Vec<NewMember>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NewMember {
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct LeaveChannelRequest {
    pub channel_id: String,
    pub user_id: String,
}
