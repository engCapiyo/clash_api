use crate::errors::{AppError, Result};
use crate::models::channel::{Channel, ChannelFixture, Message, ReplyToData};
use crate::models::user::User;
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use bson::{doc, oid::ObjectId};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;
use uuid::Uuid;

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
// BROADCAST FUNCTION
// ============================================================================

pub async fn broadcast_live_match_update(
    state: &AppState,
    channel_id: &str,
    fixture_id: &str,
    event_type: &str,
    data: serde_json::Value,
) {
    let room_key = format!("{}_{}", channel_id, fixture_id);
    let tx = state.get_or_create_broadcaster(&room_key);

    let ws_message = serde_json::json!({
        "type": event_type,
        "payload": data,
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(json) = serde_json::to_string(&ws_message) {
        let _ = tx.send(json);
        tracing::info!("📡 Broadcasted {} event to room {}", event_type, room_key);
    }
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
    let username = params
        .username
        .clone()
        .unwrap_or_else(|| "Anonymous".to_string());
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
// PER-CONNECTION LOGIC
// ============================================================================

async fn handle_socket(
    socket: WebSocket,
    channel_id: String,
    fixture_id: Option<String>,
    user_id: String,
    username: String,
    state: AppState,
) {
    // Mark user as online
    state.set_user_online(&user_id);

    let initial_room_key = match &fixture_id {
        Some(f) => format!("{}_{}", channel_id, f),
        None => format!("{}_overall", channel_id),
    };

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    let welcome = serde_json::json!({
        "type": "connected",
        "channel_id": channel_id,
        "fixture_id": fixture_id,
        "room_id": initial_room_key,
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
        "✅ WS connected: user {} to room {}",
        user_id,
        initial_room_key
    );

    let current_room: Arc<Mutex<String>> = Arc::new(Mutex::new(initial_room_key.clone()));
    let forwarder: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    let initial_handle = spawn_room_forwarder(&state, &initial_room_key, sender.clone());
    *forwarder.lock().await = Some(initial_handle);

    let sender_clone = sender.clone();
    let state_clone = state.clone();
    let current_room_clone = current_room.clone();
    let forwarder_clone = forwarder.clone();
    let user_id_clone = user_id.clone();

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                WsMessage::Text(text) => {
                    handle_incoming_message(
                        text,
                        &state_clone,
                        &sender_clone,
                        &current_room_clone,
                        &forwarder_clone,
                        &user_id_clone,
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

    let _ = &mut recv_task;
    recv_task.await.ok();

    if let Some(handle) = forwarder.lock().await.take() {
        handle.abort();
    }

    // Mark user as offline
    state.set_user_offline(&user_id);

    tracing::info!(
        "🔌 WS disconnected: user {} from room {}",
        user_id,
        current_room.lock().await
    );
}

// ============================================================================
// ROOM FORWARDER
// ============================================================================

fn spawn_room_forwarder(
    state: &AppState,
    room_key: &str,
    sender: Arc<Mutex<futures_util::stream::SplitSink<WebSocket, WsMessage>>>,
) -> tokio::task::JoinHandle<()> {
    let tx = state.get_or_create_broadcaster(room_key);
    let mut rx = tx.subscribe();

    tokio::spawn(async move {
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
    })
}

// ============================================================================
// HANDLE INCOMING MESSAGE
// ============================================================================

async fn handle_incoming_message(
    text: String,
    state: &AppState,
    sender: &Arc<Mutex<futures_util::stream::SplitSink<WebSocket, WsMessage>>>,
    current_room: &Arc<Mutex<String>>,
    forwarder: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    connection_user_id: &str,
) {
    tracing::info!("🔥 RAW WS MESSAGE: {}", text);

    let json_msg = match serde_json::from_str::<Value>(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("❌ JSON PARSE FAILED: {} | raw: {}", e, text);
            return;
        }
    };

    let message_type = json_msg.get("type").and_then(|t| t.as_str());

    match message_type {
        Some("room.join") => {
            let sender_clone = sender.clone();

            let payload = match json_msg.get("payload") {
                Some(p) => p,
                None => {
                    tracing::error!("❌ room.join missing payload");
                    return;
                }
            };

            let new_channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let new_fixture_id = payload
                .get("fixture_id")
                .and_then(|v| if v.is_null() { None } else { v.as_str() })
                .filter(|s| !s.is_empty());

            if new_channel_id.is_empty() {
                tracing::error!("❌ room.join missing channel_id");
                return;
            }

            let new_room_key = match new_fixture_id {
                Some(f) => format!("{}_{}", new_channel_id, f),
                None => format!("{}_overall", new_channel_id),
            };

            let mut current = current_room.lock().await;
            if *current == new_room_key {
                let ack = serde_json::json!({
                    "type": "room.joined",
                    "payload": { "room_id": new_room_key },
                    "timestamp": Utc::now().to_rfc3339(),
                });
                if let Ok(ack_json) = serde_json::to_string(&ack) {
                    let mut guard = sender_clone.lock().await;
                    let _ = guard.send(WsMessage::Text(ack_json)).await;
                }
                return;
            }

            tracing::info!(
                "🔀 User {} switching room: {} → {}",
                connection_user_id,
                current,
                new_room_key
            );

            if let Some(old_handle) = forwarder.lock().await.take() {
                old_handle.abort();
            }

            let sender_for_forwarder = sender.clone();
            let new_handle = spawn_room_forwarder(state, &new_room_key, sender_for_forwarder);
            *forwarder.lock().await = Some(new_handle);
            *current = new_room_key.clone();
            drop(current);

            let ack = serde_json::json!({
                "type": "room.joined",
                "payload": { "room_id": new_room_key },
                "timestamp": Utc::now().to_rfc3339(),
            });
            if let Ok(ack_json) = serde_json::to_string(&ack) {
                let mut guard = sender_clone.lock().await;
                let _ = guard.send(WsMessage::Text(ack_json)).await;
            }
        }

        // ============================================================================
        // GET COMMENTARY - For live commentary from poller (just acknowledge)
        // ============================================================================
        Some("get.commentary") => {
            tracing::info!("📖 get.commentary received");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ get.commentary missing payload");
                    return;
                }
            };

            let fixture_id = payload
                .get("fixtureId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Just acknowledge - poller sends signals separately
            let response = serde_json::json!({
                "type": "commentary.response",
                "payload": {
                    "fixture_id": fixture_id,
                    "commentary": [],
                },
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&response) {
                let mut guard = sender.lock().await;
                let _ = guard.send(WsMessage::Text(json)).await;
                tracing::info!("📤 Sent commentary response (poller handles signals)");
            }
        }

        // ============================================================================
        // GET LATEST COMMENT - For client chat messages
        // ============================================================================
        Some("get.latest.comment") => {
            tracing::info!("📖 Getting latest client chat message for fixture");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ get.latest.comment missing payload");
                    return;
                }
            };

            let fixture_id = payload
                .get("fixtureId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if fixture_id.is_empty() {
                tracing::error!("❌ get.latest.comment missing fixtureId");
                return;
            }

            // Get the channel_id from the connection context
            let current_room_lock = current_room.lock().await;
            let channel_id = current_room_lock
                .split('_')
                .next()
                .unwrap_or("")
                .to_string();
            drop(current_room_lock);

            if channel_id.is_empty() {
                tracing::error!("❌ get.latest.comment: Could not determine channel_id");
                return;
            }

            // Query latest chat message from messages collection
            let messages_col = state.db.collection::<Message>("messages");

            let filter = doc! {
                "channel_id": &channel_id,
                "fixture_id": &fixture_id,
            };

            let options = mongodb::options::FindOptions::builder()
                .sort(doc! { "sent_at": -1 })
                .limit(1)
                .build();

            match messages_col.find_one(filter).await {
                Ok(Some(msg)) => {
                    let latest_comment = serde_json::json!({
                        "id": msg.message_id,
                        "sender_id": msg.sender_id,
                        "sender_name": msg.sender_name,
                        "text": msg.text,
                        "selection": msg.selection,
                        "image_url": msg.image_url,
                        "video_url": msg.video_url,
                        "is_image": msg.is_image,
                        "is_video": msg.is_video,
                        "timestamp": msg.sent_at.to_chrono().to_rfc3339(),
                        "reply_to": msg.reply_to.map(|r| serde_json::json!({
                            "messageId": r.message_id,
                            "text": r.text,
                            "username": r.username,
                            "selection": r.selection,
                            "isMe": r.is_me,
                        })),
                    });

                    let response = serde_json::json!({
                        "type": "latest.comment.response",
                        "payload": {
                            "fixture_id": fixture_id,
                            "channel_id": channel_id,
                            "comment": latest_comment,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(json) = serde_json::to_string(&response) {
                        let mut guard = sender.lock().await;
                        let _ = guard.send(WsMessage::Text(json)).await;
                        tracing::info!("📤 Sent latest client comment response");
                    }
                }
                Ok(None) => {
                    // No comments found - send empty response
                    let response = serde_json::json!({
                        "type": "latest.comment.response",
                        "payload": {
                            "fixture_id": fixture_id,
                            "channel_id": channel_id,
                            "comment": null,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(json) = serde_json::to_string(&response) {
                        let mut guard = sender.lock().await;
                        let _ = guard.send(WsMessage::Text(json)).await;
                        tracing::info!("📤 Sent empty latest client comment response");
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Failed to fetch latest client comment: {}", e);
                }
            }
        }

        Some("chat.message") => {
            tracing::info!("✅ Matched chat.message");

            let sender_clone = sender.clone();

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ NO PAYLOAD in: {:?}", json_msg);
                    return;
                }
            };

            let channel_id = payload
                .get("channelId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let fixture_id = payload
                .get("fixtureId")
                .and_then(|v| if v.is_null() { None } else { v.as_str() })
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            let user_id = payload
                .get("userId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let username = payload
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("Anonymous")
                .to_string();

            if channel_id.is_empty() {
                tracing::error!("❌ Missing channelId in message payload");
                return;
            }

            if user_id.is_empty() {
                tracing::error!("❌ Missing userId in message payload");
                return;
            }

            tracing::info!(
                "💾 Saving to DB - channel: {}, fixture: {:?}, user: {}",
                channel_id,
                fixture_id,
                user_id
            );

            match save_message_to_database(
                state,
                &channel_id,
                &fixture_id,
                &user_id,
                &username,
                &payload,
            )
            .await
            {
                Ok(_) => tracing::info!("✅ DB SAVE SUCCESS"),
                Err(e) => {
                    tracing::error!("❌ DB SAVE FAILED: {}", e);
                    return;
                }
            }

            // Broadcast comment count update
            let room_key = match &fixture_id {
                Some(f) => format!("{}_{}", channel_id, f),
                None => format!("{}_overall", channel_id),
            };

            let room_broadcaster = state.get_or_create_broadcaster(&room_key);
            let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");

            if let Some(fixture_id) = &fixture_id {
                if let Ok(Some(cf)) = channel_fixtures_col
                    .find_one(doc! {
                        "channel_id": &channel_id,
                        "fixture_id": fixture_id,
                    })
                    .await
                {
                    let count_msg = serde_json::json!({
                        "type": "comment.count",
                        "payload": {
                            "fixture_id": fixture_id,
                            "channel_id": &channel_id,
                            "count": cf.comment_count,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(count_json) = serde_json::to_string(&count_msg) {
                        let _ = room_broadcaster.send(count_json);
                        tracing::info!("📡 Broadcasted comment.count to room: {}", room_key);
                    }
                }
            }

            // Broadcast the actual message
            let broadcast_msg = serde_json::json!({
                "type": "chat.message",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            match serde_json::to_string(&broadcast_msg) {
                Ok(broadcast_json) => {
                    let _ = room_broadcaster.send(broadcast_json);
                    tracing::info!("📡 Broadcasted chat.message to room: {}", room_key);
                }
                Err(e) => tracing::error!("❌ Serialize FAILED: {}", e),
            }
        }

        Some("typing") => {
            let sender_clone = sender.clone();

            if let Some(payload) = json_msg.get("payload") {
                let channel_id = payload
                    .get("channelId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let fixture_id = payload
                    .get("fixtureId")
                    .and_then(|v| if v.is_null() { None } else { v.as_str() })
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                if !channel_id.is_empty() {
                    let room_key = match &fixture_id {
                        Some(f) => format!("{}_{}", channel_id, f),
                        None => format!("{}_overall", channel_id),
                    };

                    let room_broadcaster = state.get_or_create_broadcaster(&room_key);

                    let broadcast_msg = serde_json::json!({
                        "type": "typing",
                        "payload": payload,
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = room_broadcaster.send(broadcast_json);
                    }
                }
            }
        }

        Some("ping") => {
            let pong = serde_json::json!({
                "type": "pong",
                "timestamp": Utc::now().to_rfc3339(),
            });
            if let Ok(pong_json) = serde_json::to_string(&pong) {
                let mut guard = sender.lock().await;
                let _ = guard.send(WsMessage::Text(pong_json)).await;
            }
        }

        Some("join.request") => {
            tracing::info!("📨 Join request via WebSocket");

            let sender_clone = sender.clone();

            if let Some(payload) = json_msg.get("payload") {
                let channel_id = payload
                    .get("channel_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let user_id = payload
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let username = payload
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");

                let channel_name = payload
                    .get("channel_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Channel");

                if channel_id.is_empty() || user_id.is_empty() {
                    tracing::warn!("⚠️ Missing channel_id or user_id in join.request");
                    return;
                }

                let channels_col = state.db.collection::<Channel>("channels");

                match channels_col
                    .find_one(doc! { "channel_id": &channel_id })
                    .await
                {
                    Ok(Some(channel)) => {
                        let admin_user_ids: Vec<String> = channel
                            .members
                            .iter()
                            .filter(|m| m.role == "admin")
                            .map(|m| m.user_id.clone())
                            .collect();

                        for admin_id in &admin_user_ids {
                            let admin_room = format!("user_{}", admin_id);
                            let admin_tx = state.get_or_create_broadcaster(&admin_room);

                            let notification = serde_json::json!({
                                "type": "join_request_received",
                                "payload": {
                                    "channel_id": channel_id,
                                    "channel_name": channel_name,
                                    "requester_id": user_id,
                                    "requester_name": username,
                                    "request_id": format!("{}_{}", user_id, channel_id),
                                    "timestamp": Utc::now().to_rfc3339(),
                                }
                            });

                            if let Ok(json) = serde_json::to_string(&notification) {
                                let _ = admin_tx.send(json);
                            }
                        }

                        let channel_room_key = format!("channel_{}", channel_id);
                        let channel_tx = state.get_or_create_broadcaster(&channel_room_key);

                        let channel_notification = serde_json::json!({
                            "type": "join_request",
                            "payload": {
                                "channel_id": channel_id,
                                "channel_name": channel_name,
                                "requester_id": user_id,
                                "requester_name": username,
                                "timestamp": Utc::now().to_rfc3339(),
                            }
                        });

                        if let Ok(json) = serde_json::to_string(&channel_notification) {
                            let _ = channel_tx.send(json);
                        }

                        let confirmation = serde_json::json!({
                            "type": "join.response",
                            "payload": {
                                "channel_id": channel_id,
                                "status": "pending",
                                "message": format!("Join request sent to admins of {}", channel_name),
                                "timestamp": Utc::now().to_rfc3339(),
                            }
                        });

                        if let Ok(json) = serde_json::to_string(&confirmation) {
                            let mut guard = sender_clone.lock().await;
                            let _ = guard.send(WsMessage::Text(json)).await;
                        }
                    }
                    Ok(None) => {
                        tracing::error!("❌ Channel not found: {}", channel_id);
                    }
                    Err(e) => {
                        tracing::error!("❌ Database error: {}", e);
                    }
                }
            }
        }

        // ============================================================================
        // VOTE EVENTS
        // ============================================================================
        Some("vote.cast") => {
            tracing::info!("🗳️ Vote cast via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ vote.cast missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let user_id = payload
                .get("user_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() || user_id.is_empty() {
                tracing::error!("❌ vote.cast missing required fields");
                return;
            }

            // Broadcast to channel room
            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "vote.cast",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted vote.cast to room: {}", room_key);
            }

            // Send to user's personal room for online check
            let user_room = format!("user_{}", user_id);
            let user_tx = state.get_or_create_broadcaster(&user_room);
            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = user_tx.send(json);
            }
        }

        Some("vote.update") => {
            tracing::info!("📊 Vote update via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ vote.update missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() {
                tracing::error!("❌ vote.update missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "vote.update",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted vote.update to room: {}", room_key);
            }
        }

        // ============================================================================
        // PLEDGE / BET EVENTS
        // ============================================================================
        Some("pledge.create") => {
            tracing::info!("💰 Pledge created via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ pledge.create missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() {
                tracing::error!("❌ pledge.create missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "pledge.create",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted pledge.create to room: {}", room_key);
            }
        }

        Some("bet.matched") => {
            tracing::info!("🤝 Bet matched via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ bet.matched missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() {
                tracing::error!("❌ bet.matched missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "bet.matched",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted bet.matched to room: {}", room_key);
            }
        }

        Some("bet.settled") => {
            tracing::info!("⚖️ Bet settled via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ bet.settled missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() {
                tracing::error!("❌ bet.settled missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "bet.settled",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted bet.settled to room: {}", room_key);
            }
        }

        Some("bet.refunded") => {
            tracing::info!("🔄 Bet refunded via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ bet.refunded missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || fixture_id.is_empty() {
                tracing::error!("❌ bet.refunded missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, fixture_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "bet.refunded",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted bet.refunded to room: {}", room_key);
            }
        }

        // ============================================================================
        // SUB-FIXTURE EVENTS
        // ============================================================================
        Some("sub_fixture.pledge") => {
            tracing::info!("📊 Sub-fixture pledge via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ sub_fixture.pledge missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let match_id = payload
                .get("match_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || match_id.is_empty() {
                tracing::error!("❌ sub_fixture.pledge missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, match_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "sub_fixture.pledge",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted sub_fixture.pledge to room: {}", room_key);
            }
        }

        Some("sub_fixture.matched") => {
            tracing::info!("🤝 Sub-fixture matched via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ sub_fixture.matched missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let match_id = payload
                .get("match_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || match_id.is_empty() {
                tracing::error!("❌ sub_fixture.matched missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, match_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "sub_fixture.matched",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted sub_fixture.matched to room: {}", room_key);
            }
        }

        Some("sub_fixture.settled") => {
            tracing::info!("⚖️ Sub-fixture settled via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ sub_fixture.settled missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let match_id = payload
                .get("match_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || match_id.is_empty() {
                tracing::error!("❌ sub_fixture.settled missing required fields");
                return;
            }

            let room_key = format!("{}_{}", channel_id, match_id);
            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "sub_fixture.settled",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted sub_fixture.settled to room: {}", room_key);
            }
        }

        // ============================================================================
        // POST / SOCIAL EVENTS
        // ============================================================================
        Some("post.like") => {
            tracing::info!("❤️ Post like via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ post.like missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let post_id = payload
                .get("post_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || post_id.is_empty() {
                tracing::error!("❌ post.like missing required fields");
                return;
            }

            let room_key = match fixture_id.is_empty() {
                false => format!("{}_{}", channel_id, fixture_id),
                true => format!("{}_overall", channel_id),
            };

            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "post.like",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted post.like to room: {}", room_key);
            }
        }

        Some("post.comment") => {
            tracing::info!("💬 Post comment via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ post.comment missing payload");
                    return;
                }
            };

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let fixture_id = payload
                .get("fixture_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let post_id = payload
                .get("post_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if channel_id.is_empty() || post_id.is_empty() {
                tracing::error!("❌ post.comment missing required fields");
                return;
            }

            let room_key = match fixture_id.is_empty() {
                false => format!("{}_{}", channel_id, fixture_id),
                true => format!("{}_overall", channel_id),
            };

            let room_broadcaster = state.get_or_create_broadcaster(&room_key);

            let broadcast_msg = serde_json::json!({
                "type": "post.comment",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = room_broadcaster.send(json);
                tracing::info!("📡 Broadcasted post.comment to room: {}", room_key);
            }
        }

        Some("comrade.added") => {
            tracing::info!("👥 Comrade added via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ comrade.added missing payload");
                    return;
                }
            };

            let target_user_id = payload
                .get("target_user_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if target_user_id.is_empty() {
                tracing::error!("❌ comrade.added missing target_user_id");
                return;
            }

            // Send to target user's personal room
            let user_room = format!("user_{}", target_user_id);
            let user_tx = state.get_or_create_broadcaster(&user_room);

            let broadcast_msg = serde_json::json!({
                "type": "comrade.added",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = user_tx.send(json);
                tracing::info!("📡 Broadcasted comrade.added to user: {}", target_user_id);
            }
        }

        // ============================================================================
        // CHANNEL JOIN EVENTS
        // ============================================================================
        Some("join.approved") => {
            tracing::info!("✅ Join approved via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ join.approved missing payload");
                    return;
                }
            };

            let user_id = payload
                .get("user_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let channel_id = payload
                .get("channel_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if user_id.is_empty() || channel_id.is_empty() {
                tracing::error!("❌ join.approved missing required fields");
                return;
            }

            // Send to user's personal room
            let user_room = format!("user_{}", user_id);
            let user_tx = state.get_or_create_broadcaster(&user_room);

            let broadcast_msg = serde_json::json!({
                "type": "join.approved",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = user_tx.send(json);
                tracing::info!("📡 Broadcasted join.approved to user: {}", user_id);
            }

            // Also broadcast to channel room
            let channel_room = format!("channel_{}", channel_id);
            let channel_tx = state.get_or_create_broadcaster(&channel_room);
            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = channel_tx.send(json);
            }
        }

        Some("join.rejected") => {
            tracing::info!("❌ Join rejected via WebSocket");

            let payload = match json_msg.get("payload") {
                Some(p) => p.clone(),
                None => {
                    tracing::error!("❌ join.rejected missing payload");
                    return;
                }
            };

            let user_id = payload
                .get("user_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if user_id.is_empty() {
                tracing::error!("❌ join.rejected missing user_id");
                return;
            }

            let user_room = format!("user_{}", user_id);
            let user_tx = state.get_or_create_broadcaster(&user_room);

            let broadcast_msg = serde_json::json!({
                "type": "join.rejected",
                "payload": payload,
                "timestamp": Utc::now().to_rfc3339(),
            });

            if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                let _ = user_tx.send(json);
                tracing::info!("📡 Broadcasted join.rejected to user: {}", user_id);
            }
        }

        _ => {
            tracing::warn!("⚠️ UNMATCHED type: {:?}", message_type);
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
    payload: &serde_json::Value,
) -> Result<()> {
    let messages_col = state.db.collection::<Message>("messages");
    let channel_fixtures_col = state.db.collection::<ChannelFixture>("channel_fixtures");
    let channels_col = state.db.collection::<Channel>("channels");

    let message_id = payload
        .get("messageId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4()));

    let text = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let selection = payload
        .get("selection")
        .and_then(|v| v.as_str())
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

    let reply_to = payload
        .get("replyTo")
        .and_then(|v| v.as_object())
        .map(|obj| {
            let reply_to_id = obj
                .get("messageId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let reply_to_text = obj
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let reply_to_username = obj
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let reply_to_selection = obj
                .get("selection")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let is_me = obj.get("isMe").and_then(|v| v.as_bool()).unwrap_or(false);

            ReplyToData {
                message_id: reply_to_id,
                text: reply_to_text,
                username: reply_to_username,
                selection: reply_to_selection,
                is_me: false,
                image_url: None,
                video_url: None,
                is_image: false,
                is_video: false,
            }
        });

    let now_bson = bson::DateTime::now();

    let message = Message {
        id: Some(ObjectId::new()),
        channel_id: channel_id.to_string(),
        fixture_id: fixture_id.clone(),
        sender_id: user_id.to_string(),
        sender_name: username.to_string(),
        text: text.clone(),
        caption: None,
        sent_at: now_bson,
        message_id: Some(message_id.clone()),
        selection: selection,
        temp_id: None,
        image_url: image_url,
        image_public_id: None,
        image_caption: None,
        is_image: is_image,
        video_url: video_url,
        video_public_id: None,
        video_thumbnail_url: None,
        video_caption: None,
        video_duration: None,
        video_size: None,
        is_video: is_video,
        reply_to: reply_to,
        reply_to_id: None,
    };

    messages_col.insert_one(&message).await?;

    // Update channel fixture counts
    if let Some(fixture_id) = fixture_id {
        let update_result = channel_fixtures_col
            .update_one(
                doc! {
                    "channel_id": channel_id,
                    "fixture_id": fixture_id,
                },
                doc! { "$inc": { "comment_count": 1 } },
            )
            .await;

        match update_result {
            Ok(result) => {
                if result.modified_count > 0 {
                    tracing::info!("📊 Incremented comment_count for fixture {}", fixture_id);
                } else {
                    tracing::warn!(
                        "⚠️ No channel_fixture found for channel {} fixture {}",
                        channel_id,
                        fixture_id
                    );
                }
            }
            Err(e) => {
                tracing::error!("❌ Failed to increment comment_count: {}", e);
            }
        }
    }

    // Update channel activity
    let now_chrono = chrono::Utc::now();
    let now_bson_update = bson::DateTime::from_chrono(now_chrono);

    channels_col
        .update_one(
            doc! { "channel_id": channel_id },
            doc! {
                "$inc": {
                    "activity.total_messages": 1,
                    "activity.messages_this_week": 1,
                },
                "$set": {
                    "activity.last_message_at": now_bson_update,
                },
            },
        )
        .await?;

    // Update member message count
    channels_col
        .update_one(
            doc! {
                "channel_id": channel_id,
                "members.user_id": user_id,
            },
            doc! {
                "$inc": { "members.$.msg_count": 1 },
                "$set": { "members.$.last_active_at": now_bson_update },
            },
        )
        .await?;

    Ok(())
}
