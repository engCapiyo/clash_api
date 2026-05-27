use axum::{extract::State, Json};
use mongodb::bson::{doc, DateTime};
use serde_json::json;
use uuid::Uuid;

use crate::models::chat::{
    Channel, ChannelActivity, ChannelFixture, ChannelMember, Message, Payout, Vote, VoteCounts,
};
use crate::{AppError, AppState};

// ============================================================================
// CREATE CHANNEL
// ============================================================================

pub async fn create_channel_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateChannelRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channels_col = state.db.collection::<Channel>("channels");
    let now = DateTime::now();
    let channel_id = Uuid::new_v4().to_string();

    let channel = Channel {
        id: None,
        channel_id: channel_id.clone(),
        name: payload.name,
        created_by: payload.created_by.clone(),
        created_at: now,
        members: vec![ChannelMember {
            user_id: payload.created_by,
            username: payload.created_by_username,
            role: "admin".to_string(),
            joined_at: now,
            season_points: 0,
            correct_votes: 0,
            total_votes: 0,
            msg_count: 0,
        }],
        activity: ChannelActivity {
            total_messages: 0,
            messages_this_week: 0,
            week_reset_at: now,
            last_message_at: None,
        },
        season: payload.season,
        member_count: 1,
    };

    channels_col.insert_one(channel).await?;

    Ok(Json(json!({
        "success": true,
        "channel_id": channel_id,
    })))
}

// ============================================================================
// INITIALIZE FIXTURE CHAT
// ============================================================================

pub async fn initialize_fixture_chat_handler(
    State(state): State<AppState>,
    Json(payload): Json<InitializeFixtureChatRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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
        .find_one(doc! { "_id": &payload.fixture_id })
        .await?
        .ok_or_else(|| AppError::NotFound("Fixture not found".to_string()))?;

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
// SEND MESSAGE
// ============================================================================

pub async fn send_message_handler(
    State(state): State<AppState>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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

pub async fn get_messages_handler(
    State(state): State<AppState>,
    Json(payload): Json<GetMessagesRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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
            "fixture_id": null as Option<String>,
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

// ============================================================================
// CAST VOTE
// ============================================================================

pub async fn cast_vote_handler(
    State(state): State<AppState>,
    Json(payload): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
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
        return Err(AppError::BadRequest("Already voted".to_string()));
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
        _ => return Err(AppError::BadRequest("Invalid selection".to_string())),
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
// REQUEST TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub created_by: String,
    pub created_by_username: String,
    pub season: String,
}

#[derive(Debug, Deserialize)]
pub struct InitializeFixtureChatRequest {
    pub channel_id: String,
    pub fixture_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct GetMessagesRequest {
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CastVoteRequest {
    pub channel_id: String,
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String,
}
