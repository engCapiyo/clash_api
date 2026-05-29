use crate::models::channel::{Channel, ChannelFixture, Message};
use crate::models::game::{CommentaryEntry, Game};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use bson::{doc, oid::ObjectId, DateTime as BsonDateTime};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;

use crate::state::AppState;

// ========== QUERY PARAMS ==========
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    #[serde(rename = "roomId")]
    pub room_id: String,
    pub user_id: String,
    pub username: Option<String>,
}

// ========== WEB SOCKET MESSAGE TYPES ==========
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WSMessage {
    #[serde(rename = "chat.message")]
    ChatMessage {
        payload: ChatMessagePayload,
        timestamp: String,
    },
    #[serde(rename = "typing")]
    Typing {
        payload: TypingPayload,
        timestamp: String,
    },
    #[serde(rename = "presence")]
    Presence {
        payload: PresencePayload,
        timestamp: String,
    },
    #[serde(rename = "vote.update")]
    VoteUpdate {
        payload: VoteUpdatePayload,
        timestamp: String,
    },
    #[serde(rename = "like")]
    Like {
        payload: LikePayload,
        timestamp: String,
    },
    #[serde(rename = "pong")]
    Pong { timestamp: String },
    #[serde(rename = "connected")]
    Connected { room_id: String, timestamp: String },
}

// ========== PAYLOAD STRUCTURES ==========

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessagePayload {
    pub roomId: String,
    pub channelId: String,
    pub fixtureId: Option<String>,
    pub message: String,
    pub fromUserId: String,
    pub username: String,
    pub selection: Option<String>,
    pub messageId: String,
    pub timestamp: String,
    pub replyTo: Option<ReplyData>,
    pub imageUrl: Option<String>,
    pub videoUrl: Option<String>,
    pub isImage: Option<bool>,
    pub isVideo: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplyData {
    pub messageId: String,
    pub text: String,
    pub username: String,
    pub selection: Option<String>,
    pub isMe: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LikePayload {
    pub fixtureId: String,
    pub userId: String,
    pub username: String,
    pub action: String,
    pub totalLikes: i64,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TypingPayload {
    pub roomId: String,
    pub isTyping: bool,
    pub fromUserId: String,
    pub username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PresencePayload {
    pub user_id: String,
    pub username: String,
    pub status: String,
    pub room_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoteUpdatePayload {
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String,
    pub home_votes: i64,
    pub away_votes: i64,
    pub draw_votes: i64,
}

// ========== UPGRADE HANDLER ==========
pub async fn ws_comments_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let room_id = params.room_id.clone();
    let user_id = params.user_id.clone();
    let username = params.username.clone();

    tracing::info!(
        "🔌 WS upgrade request for room: {}, user: {}",
        room_id,
        user_id
    );

    ws.on_upgrade(move |socket| handle_socket(socket, room_id, user_id, username, state))
}

// ========== PARSE ROOM ID ==========
fn parse_room_id(room_id: &str) -> (String, Option<String>) {
    if room_id.ends_with("_overall") {
        let channel_id = room_id.trim_end_matches("_overall").to_string();
        (channel_id, None)
    } else {
        let parts: Vec<&str> = room_id.split('_').collect();
        if parts.len() >= 2 {
            let channel_id = parts[0].to_string();
            let fixture_id = Some(parts[1..].join("_"));
            (channel_id, fixture_id)
        } else {
            (room_id.to_string(), None)
        }
    }
}

// ========== PER-CONNECTION LOGIC ==========
async fn handle_socket(
    socket: WebSocket,
    room_id: String,
    user_id: String,
    username: Option<String>,
    state: AppState,
) {
    let username = username.unwrap_or_else(|| "Anonymous".to_string());
    let (channel_id, fixture_id) = parse_room_id(&room_id);

    let tx = state.get_or_create_broadcaster(&room_id);
    let mut rx = tx.subscribe();

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let sender_clone = sender.clone();

    // Clone values for use in closures
    let room_id_clone = room_id.clone();
    let channel_id_clone = channel_id.clone();
    let fixture_id_clone = fixture_id.clone();
    let user_id_clone = user_id.clone();
    let username_clone = username.clone();

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "connected",
        "room_id": room_id,
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(welcome_json) = serde_json::to_string(&welcome) {
        let mut sender_guard = sender.lock().await;
        if sender_guard
            .send(WsMessage::Text(welcome_json))
            .await
            .is_err()
        {
            return;
        }
    }

    // Broadcast user online presence
    let presence = serde_json::json!({
        "type": "presence",
        "payload": {
            "user_id": user_id_clone,
            "username": username_clone,
            "status": "online",
            "room_id": room_id_clone,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(presence_json) = serde_json::to_string(&presence) {
        let _ = tx.send(presence_json);
    }

    tracing::info!(
        "✅ WS connected: user {} to room {}",
        user_id_clone,
        room_id_clone
    );

    let room_id_for_send = room_id_clone.clone();
    let channel_id_for_save = channel_id_clone.clone();
    let fixture_id_for_save = fixture_id_clone.clone();

    // Task 1: Forward broadcast messages to this client
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let mut sender_guard = sender.lock().await;
                    if sender_guard.send(WsMessage::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WS client lagged, skipped {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Task 2: Handle incoming messages
    let state_clone = state.clone();
    let tx_clone = tx.clone();
    let room_id_recv = room_id_clone.clone();
    let channel_id_recv = channel_id_clone.clone();
    let fixture_id_recv = fixture_id_clone.clone();
    let user_id_recv = user_id_clone.clone();
    let username_recv = username_clone.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                WsMessage::Text(text) => {
                    handle_incoming_message(
                        text,
                        &state_clone,
                        &room_id_recv,
                        &channel_id_recv,
                        &fixture_id_recv,
                        &user_id_recv,
                        &username_recv,
                        &tx_clone,
                    )
                    .await;
                }
                WsMessage::Close(_) => break,
                WsMessage::Ping(_) => {
                    let pong = serde_json::json!({
                        "type": "pong",
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(pong_json) = serde_json::to_string(&pong) {
                        let mut sender_guard = sender_clone.lock().await;
                        let _ = sender_guard.send(WsMessage::Text(pong_json)).await;
                    }
                }
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    // Broadcast user offline presence
    let offline_presence = serde_json::json!({
        "type": "presence",
        "payload": {
            "user_id": user_id_clone,
            "username": username_clone,
            "status": "offline",
            "room_id": room_id_for_send,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(offline_json) = serde_json::to_string(&offline_presence) {
        let _ = tx.send(offline_json);
    }

    tracing::info!("🔌 WS disconnected for room: {}", room_id_clone);
}

async fn handle_incoming_message(
    text: String,
    state: &AppState,
    room_id: &str,
    channel_id: &str,
    fixture_id: &Option<String>,
    user_id: &str,
    username: &str,
    broadcaster: &tokio::sync::broadcast::Sender<String>,
) {
    if let Ok(json_msg) = serde_json::from_str::<Value>(&text) {
        let message_type = json_msg.get("type").and_then(|t| t.as_str());

        match message_type {
            Some("chat.message") => {
                if let Some(payload) = json_msg.get("payload") {
                    let payload_clone = payload.clone();

                    tracing::info!(
                        "📨 Received chat.message from user {} in room {}",
                        user_id,
                        room_id
                    );

                    // Save message to messages collection
                    if let Err(e) = save_message_to_database(
                        state,
                        channel_id,
                        fixture_id,
                        user_id,
                        username,
                        &payload_clone,
                    )
                    .await
                    {
                        tracing::error!("Failed to save message: {}", e);
                        return;
                    }

                    // Broadcast the message
                    let broadcast_msg = serde_json::json!({
                        "type": "chat.message",
                        "payload": payload_clone,
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                        tracing::info!("📡 Broadcasted chat.message to room {}", room_id);
                    }
                }
            }

            Some("typing") => {
                if let Some(payload) = json_msg.get("payload") {
                    let broadcast_msg = serde_json::json!({
                        "type": "typing",
                        "payload": payload,
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                    }
                }
            }

            Some("room.join") => {
                tracing::info!("User {} joined room {}", user_id, room_id);
            }

            Some("room.leave") => {
                tracing::info!("User {} left room {}", user_id, room_id);
            }

            Some("ping") => {
                let pong = serde_json::json!({
                    "type": "pong",
                    "timestamp": Utc::now().to_rfc3339(),
                });
                if let Ok(pong_json) = serde_json::to_string(&pong) {
                    let _ = broadcaster.send(pong_json);
                }
            }

            _ => {
                tracing::debug!("Unknown message type: {:?}", message_type);
            }
        }
    }
}

// ============================================================================
// CHANNEL-BASED MESSAGE STORAGE
// ============================================================================

async fn save_message_to_database(
    state: &AppState,
    channel_id: &str,
    fixture_id: &Option<String>,
    user_id: &str,
    username: &str,
    payload: &Value,
) -> Result<(), String> {
    let messages_col = state.db.collection::<Message>("messages");
    let channels_col = state.db.collection::<Channel>("channels");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

    let now = BsonDateTime::now();

    let message_text = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let selection = payload
        .get("selection")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Create message document
    let message = Message {
        id: None,
        channel_id: channel_id.to_string(),
        fixture_id: fixture_id.clone(),
        sender_id: user_id.to_string(),
        sender_name: username.to_string(),
        text: message_text.clone(),
        sent_at: now,
    };

    // Insert message
    messages_col
        .insert_one(&message)
        .await
        .map_err(|e| format!("Failed to insert message: {}", e))?;
    tracing::info!(
        "✅ Message saved to messages collection with channel_id: {}",
        channel_id
    );

    // Update channel activity counters
    channels_col
        .update_one(
            doc! { "channel_id": channel_id },
            doc! {
                "$inc": {
                    "activity.total_messages": 1,
                    "activity.messages_this_week": 1,
                },
                "$set": { "activity.last_message_at": now }
            },
        )
        .await
        .map_err(|e| format!("Failed to update channel activity: {}", e))?;
    tracing::info!("✅ Updated channel activity for channel: {}", channel_id);

    // Update member message count
    channels_col
        .update_one(
            doc! {
                "channel_id": channel_id,
                "members.user_id": user_id,
            },
            doc! { "$inc": { "members.$.msg_count": 1 } },
        )
        .await
        .map_err(|e| format!("Failed to update member count: {}", e))?;
    tracing::info!("✅ Updated member msg_count for user: {}", user_id);

    // Update fixture last message if this is a fixture chat
    if let Some(fix_id) = fixture_id {
        channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": channel_id,
                    "fixture_id": fix_id,
                },
                doc! {
                    "$set": {
                        "last_message": &message_text,
                        "last_message_at": now,
                        "last_sender": username,
                    }
                },
            )
            .await
            .map_err(|e| format!("Failed to update fixture last message: {}", e))?;
        tracing::info!("✅ Updated fixture last_message for fixture: {}", fix_id);
    }

    Ok(())
}

// ============================================================================
// BROADCAST HELPER (kept for compatibility)
// ============================================================================

pub async fn broadcast_live_match_update(
    state: &AppState,
    fixture_id: &str,
    event_type: &str,
    data: serde_json::Value,
) {
    let tx = state.get_or_create_broadcaster(fixture_id);

    let ws_message = serde_json::json!({
        "type": event_type,
        "payload": data,
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(json) = serde_json::to_string(&ws_message) {
        let _ = tx.send(json);
        tracing::info!(
            "📡 Broadcasted {} event for fixture {}",
            event_type,
            fixture_id
        );
    }
}
