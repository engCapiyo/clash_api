use crate::models::channel::{Channel, ChannelFixture, Message, ReplyToData};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use bson::{doc, DateTime as BsonDateTime};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;

use crate::state::AppState;

// ============================================================================
// QUERY PARAMS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub user_id: String,
    pub username: Option<String>,
    pub channel_id: String,
    pub fixture_id: Option<String>,
}

// ============================================================================
// UPGRADE HANDLER
// ============================================================================

pub async fn ws_comments_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let user_id = params.user_id.clone();
    let username = params.username.clone();
    let channel_id = params.channel_id.clone();
    let fixture_id = if params.fixture_id.as_deref() == Some("") {
        None
    } else {
        params.fixture_id.clone()
    };

    tracing::info!(
        "🔌 WS upgrade — channel: {}, fixture: {:?}, user: {}",
        channel_id,
        fixture_id,
        user_id
    );

    ws.on_upgrade(move |socket| {
        handle_socket(socket, channel_id, fixture_id, user_id, username, state)
    })
}

// ============================================================================
// PARSE ROOM ID
// ============================================================================

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

// ============================================================================
// PER-CONNECTION LOGIC
// ============================================================================

async fn handle_socket(
    socket: WebSocket,
    channel_id: String,
    fixture_id: Option<String>,
    user_id: String,
    username: Option<String>,
    state: AppState,
) {
    let username = username.unwrap_or_else(|| "Anonymous".to_string());

    // Build a display room string just for logging
    let room_display = match &fixture_id {
        Some(f) => format!("{}_{}", channel_id, f),
        None => format!("{}_overall", channel_id),
    };

    // Use channel_id as broadcaster key
    let broadcaster_key = room_display.clone();
    let tx = state.get_or_create_broadcaster(&broadcaster_key);
    let mut rx = tx.subscribe();

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let sender_clone = sender.clone();

    // Welcome message
    let welcome = serde_json::json!({
        "type": "connected",
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

    tracing::info!(
        "✅ WS connected: user {} to channel {} fixture {:?}",
        user_id,
        channel_id,
        fixture_id
    );

    let channel_id_clone = channel_id.clone();
    let fixture_id_clone = fixture_id.clone();
    let user_id_clone = user_id.clone();
    let username_clone = username.clone();

    // Task 1: forward broadcasts to client
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let mut guard = sender.lock().await;
                    if guard.send(WsMessage::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WS client lagged {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Task 2: handle incoming
    let state_clone = state.clone();
    let tx_clone = tx.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                WsMessage::Text(text) => {
                    handle_incoming_message(
                        text,
                        &state_clone,
                        &channel_id_clone,
                        &fixture_id_clone,
                        &user_id_clone,
                        &username_clone,
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
                        let mut guard = sender_clone.lock().await;
                        let _ = guard.send(WsMessage::Text(pong_json)).await;
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

    tracing::info!(
        "🔌 WS disconnected: user {} from channel {}",
        user_id,
        channel_id
    );
}
// ============================================================================
// HANDLE INCOMING MESSAGE
// ============================================================================

async fn handle_incoming_message(
    text: String,
    state: &AppState,
    channel_id: &str,
    fixture_id: &Option<String>,
    user_id: &str,
    username: &str,
    broadcaster: &tokio::sync::broadcast::Sender<String>,
) {
    tracing::info!("🔥 RAW WS MESSAGE: {}", text);
    tracing::info!(
        "🔥 channel_id: {}, fixture_id: {:?}, user_id: {}",
        channel_id,
        fixture_id,
        user_id
    );

    let json_msg = match serde_json::from_str::<Value>(&text) {
        Ok(v) => {
            tracing::info!("✅ JSON parsed: {:?}", v);
            v
        }
        Err(e) => {
            tracing::error!("❌ JSON PARSE FAILED: {} | raw: {}", e, text);
            return;
        }
    };

    let message_type = json_msg.get("type").and_then(|t| t.as_str());
    tracing::info!("🔥 message_type: {:?}", message_type);

    match message_type {
        Some("chat.message") => {
            tracing::info!("✅ Matched chat.message");

            let payload = match json_msg.get("payload") {
                Some(p) => {
                    tracing::info!("✅ Payload: {:?}", p);
                    p.clone()
                }
                None => {
                    tracing::error!("❌ NO PAYLOAD in: {:?}", json_msg);
                    return;
                }
            };

            tracing::info!("💾 Saving to DB...");

            match save_message_to_database(
                state, channel_id, fixture_id, user_id, username, &payload,
            )
            .await
            {
                Ok(_) => tracing::info!("✅ DB SAVE SUCCESS"),
                Err(e) => {
                    tracing::error!("❌ DB SAVE FAILED: {}", e);
                    return;
                }
            }

            let broadcast_msg = serde_json::json!({
                "type": "chat.message",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            match serde_json::to_string(&broadcast_msg) {
                Ok(broadcast_json) => match broadcaster.send(broadcast_json) {
                    Ok(n) => tracing::info!("📡 Broadcasted to {} receivers", n),
                    Err(e) => tracing::error!("❌ Broadcast FAILED: {}", e),
                },
                Err(e) => tracing::error!("❌ Serialize FAILED: {}", e),
            }
        }

        Some("typing") => {
            tracing::info!("✅ Matched typing");
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
            tracing::info!("👋 User {} joined channel {}", user_id, channel_id);
        }

        Some("room.leave") => {
            tracing::info!("👋 User {} left channel {}", user_id, channel_id);
        }

        Some("ping") => {
            tracing::info!("🏓 Ping from {}", user_id);
            let pong = serde_json::json!({
                "type": "pong",
                "timestamp": Utc::now().to_rfc3339(),
            });
            if let Ok(pong_json) = serde_json::to_string(&pong) {
                let _ = broadcaster.send(pong_json);
            }
        }

        other => {
            tracing::warn!("⚠️ UNMATCHED type: {:?} | full: {}", other, text);
        }
    }
}
// ============================================================================
// SAVE MESSAGE TO DATABASE
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

    let message_id = payload
        .get("messageId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let selection = payload
        .get("selection")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let image_url = payload
        .get("imageUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let video_url = payload
        .get("videoUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let is_image = payload
        .get("isImage")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let is_video = payload
        .get("isVideo")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let reply_to: Option<ReplyToData> = payload
        .get("replyTo")
        .and_then(|v| if v.is_null() { None } else { Some(v) })
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let message = Message {
        id: None,
        channel_id: channel_id.to_string(),
        fixture_id: fixture_id.clone(),
        sender_id: user_id.to_string(),
        sender_name: username.to_string(),
        text: message_text.clone(),
        sent_at: now,
        message_id,
        selection,
        image_url,
        video_url,
        is_image,
        is_video,
        reply_to,
    };

    messages_col
        .insert_one(&message)
        .await
        .map_err(|e| format!("Failed to insert message: {}", e))?;

    tracing::info!(
        "✅ Message saved — user: {}, channel: {}, fixture: {:?}",
        user_id,
        channel_id,
        fixture_id
    );

    // Update channel activity
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

    // Update fixture last message if applicable
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
    }

    Ok(())
}

// ============================================================================
// BROADCAST HELPER
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
