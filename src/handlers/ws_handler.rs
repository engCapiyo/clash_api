use crate::models::game::{CommentaryEntry, Game};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
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

use crate::models::vote::{Comment, ReplyData};
use crate::state::AppState;

// ========== QUERY PARAMS ==========
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "userId")]
    pub user_id: String,

    #[serde(rename = "username")]
    pub username: Option<String>,
}

// ========== WEB SOCKET MESSAGE TYPES ==========
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WSMessage {
    #[serde(rename = "comment.new")]
    CommentNew {
        payload: CommentPayload,
        timestamp: String,
    },
    #[serde(rename = "chat.message")]
    ChatMessage {
        payload: ChatMessagePayload,
        timestamp: String,
    },
    #[serde(rename = "comment.count")]
    CommentCount {
        payload: CommentCountPayload,
        timestamp: String,
    },
    #[serde(rename = "typing")]
    Typing {
        payload: TypingPayload,
        timestamp: String,
    },
    #[serde(rename = "comment.seen")]
    CommentSeen {
        payload: CommentReadReceipt,
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
    #[serde(rename = "match.goal")]
    MatchGoal {
        payload: GoalPayload,
        timestamp: String,
    },
    #[serde(rename = "match.score")]
    MatchScore {
        payload: ScorePayload,
        timestamp: String,
    },
    #[serde(rename = "match.card")]
    MatchCard {
        payload: CardPayload,
        timestamp: String,
    },
    #[serde(rename = "match.half_time")]
    MatchHalfTime {
        payload: HalfTimePayload,
        timestamp: String,
    },
    #[serde(rename = "match.full_time")]
    MatchFullTime {
        payload: FullTimePayload,
        timestamp: String,
    },
    #[serde(rename = "match.statistics")]
    MatchStatistics {
        payload: StatisticsPayload,
        timestamp: String,
    },
    #[serde(rename = "match.status")]
    MatchStatus {
        payload: StatusPayload,
        timestamp: String,
    },
    #[serde(rename = "commentary.new")]
    CommentaryNew {
        payload: CommentaryEntry,
        timestamp: String,
    },
    #[serde(rename = "pong")]
    Pong { timestamp: String },
    #[serde(rename = "connected")]
    Connected {
        fixture_id: String,
        timestamp: String,
    },
}

// ========== PAYLOAD STRUCTURES ==========
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommentPayload {
    pub comment_id: String,
    pub voter_id: String,
    pub username: String,
    pub fixture_id: String,
    pub selection: String,
    pub comment: String,
    pub timestamp: String,
    pub likes: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessagePayload {
    pub toUserId: Option<String>,
    pub message: String,
    pub fixtureId: String,
    pub fromUserId: String,
    pub username: String,
    pub selection: String,
    pub messageId: String,
    pub timestamp: String,
    pub replyTo: Option<ReplyData>,
    pub imageUrl: Option<String>,
    pub videoUrl: Option<String>,
    pub isImage: Option<bool>,
    pub isVideo: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoomMessagePayload {
    pub roomId: String,
    pub message: String,
    pub fromUserId: String,
    pub username: String,
    pub selection: String,
    pub messageId: String,
    pub timestamp: String,
    pub replyTo: Option<ReplyData>,
    pub imageUrl: Option<String>,
    pub videoUrl: Option<String>,
    pub isImage: Option<bool>,
    pub isVideo: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommentCountPayload {
    pub fixtureId: String,
    pub count: i64,
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
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub is_typing: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommentReadReceipt {
    pub comment_id: String,
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub seen_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PresencePayload {
    pub user_id: String,
    pub username: String,
    pub status: String,
    pub fixture_id: String,
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

// ========== LIVE MATCH PAYLOADS ==========
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GoalPayload {
    pub fixture_id: String,
    pub scorer: String,
    pub scored_team: String,
    pub home_score: i32,
    pub away_score: i32,
    pub minute: i32,
    pub player: Option<String>,
    pub score_display: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScorePayload {
    pub fixture_id: String,
    pub home_score: i32,
    pub away_score: i32,
    pub minute: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CardPayload {
    pub fixture_id: String,
    pub card_type: String,
    pub team: String,
    pub player: String,
    pub minute: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HalfTimePayload {
    pub fixture_id: String,
    pub home_score: i32,
    pub away_score: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FullTimePayload {
    pub fixture_id: String,
    pub home_score: i32,
    pub away_score: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatisticsPayload {
    pub fixture_id: String,
    pub minute: i32,
    pub stats: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatusPayload {
    pub fixture_id: String,
    pub status: String,
    pub time_elapsed: i32,
}

// ========== UPGRADE HANDLER ==========
pub async fn ws_comments_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let fixture_id = params.fixture_id.clone();
    let user_id = params.user_id.clone();
    let username = params.username.unwrap_or_else(|| "Anonymous".to_string());

    tracing::info!(
        "🔌 WS upgrade request for fixture: {}, user: {}",
        fixture_id,
        user_id
    );

    ws.on_upgrade(move |socket| handle_socket(socket, fixture_id, user_id, username, state))
}

// ========== PER-CONNECTION LOGIC ==========
async fn handle_socket(
    socket: WebSocket,
    fixture_id: String,
    user_id: String,
    username: String,
    state: AppState,
) {
    let tx = state.get_or_create_broadcaster(&fixture_id);
    let mut rx = tx.subscribe();

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let sender_clone = sender.clone();

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "connected",
        "fixture_id": fixture_id,
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(welcome_json) = serde_json::to_string(&welcome) {
        let mut sender_guard = sender.lock().await;
        if sender_guard
            .send(Message::Text(welcome_json))
            .await
            .is_err()
        {
            return;
        }
    }

    // Send current match state
    send_current_match_state(&state, &fixture_id, &sender).await;

    // Broadcast user online presence
    let presence = serde_json::json!({
        "type": "presence",
        "payload": {
            "user_id": user_id,
            "username": username,
            "status": "online",
            "fixture_id": fixture_id,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(presence_json) = serde_json::to_string(&presence) {
        let _ = tx.send(presence_json);
    }

    tracing::info!(
        "✅ WS connected: user {} to fixture {}",
        user_id,
        fixture_id
    );

    let fixture_id_for_send = fixture_id.clone();
    let fixture_id_for_recv = fixture_id.clone();
    let user_id_for_recv = user_id.clone();
    let username_for_recv = username.clone();

    // Task 1: Forward broadcast messages to this client
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let mut sender_guard = sender.lock().await;
                    if sender_guard.send(Message::Text(msg)).await.is_err() {
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

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    handle_incoming_message(
                        text,
                        &state_clone,
                        &fixture_id_for_recv,
                        &user_id_for_recv,
                        &username_for_recv,
                        &tx_clone,
                    )
                    .await;
                }
                Message::Close(_) => break,
                Message::Ping(_) => {
                    let pong = serde_json::json!({
                        "type": "pong",
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(pong_json) = serde_json::to_string(&pong) {
                        let mut sender_guard = sender_clone.lock().await;
                        let _ = sender_guard.send(Message::Text(pong_json)).await;
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
            "user_id": user_id,
            "username": username,
            "status": "offline",
            "fixture_id": fixture_id_for_send,
        },
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Ok(offline_json) = serde_json::to_string(&offline_presence) {
        let _ = tx.send(offline_json);
    }

    tracing::info!("🔌 WS disconnected for fixture: {}", fixture_id);
}

async fn handle_incoming_message(
    text: String,
    state: &AppState,
    fixture_id: &str,
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
                    let fixture_id_from_payload = payload
                        .get("fixtureId")
                        .and_then(|v| v.as_str())
                        .unwrap_or(fixture_id);

                    tracing::info!(
                        "📨 Received chat.message from user {} in fixture {}",
                        user_id,
                        fixture_id_from_payload
                    );

                    if let Err(e) = save_comment_to_database(state, payload).await {
                        tracing::error!("Failed to save comment: {}", e);
                        return;
                    }

                    let total_comments = get_comment_count(state, fixture_id_from_payload).await;
                    tracing::info!("📊 Total comments after save: {}", total_comments);

                    let broadcast_msg = serde_json::json!({
                        "type": "chat.message",
                        "payload": payload_clone,
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                        tracing::info!("📡 Broadcasted chat.message");
                    }

                    let comment_count_msg = serde_json::json!({
                        "type": "comment.count",
                        "payload": {
                            "fixtureId": fixture_id_from_payload,
                            "count": total_comments,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(count_json) = serde_json::to_string(&comment_count_msg) {
                        let _ = broadcaster.send(count_json);
                        tracing::info!("📡 Broadcasted comment.count: {}", total_comments);
                    }
                }
            }

            Some("message.delete") => {
                if let Some(payload) = json_msg.get("payload") {
                    let message_id = payload
                        .get("messageId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let fixture_id_from_payload = payload
                        .get("fixtureId")
                        .and_then(|v| v.as_str())
                        .unwrap_or(fixture_id);

                    tracing::info!("📨 Received message.delete: {}", message_id);

                    if let Err(e) = delete_comment_from_database(state, message_id).await {
                        tracing::error!("Failed to delete: {}", e);
                        return;
                    }

                    let total_comments = get_comment_count(state, fixture_id_from_payload).await;

                    let delete_msg = serde_json::json!({
                        "type": "message.delete",
                        "payload": {
                            "messageId": message_id,
                            "fixtureId": fixture_id_from_payload,
                            "deletedBy": user_id,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(delete_json) = serde_json::to_string(&delete_msg) {
                        let _ = broadcaster.send(delete_json);
                    }

                    let comment_count_msg = serde_json::json!({
                        "type": "comment.count",
                        "payload": {
                            "fixtureId": fixture_id_from_payload,
                            "count": total_comments,
                        },
                        "timestamp": Utc::now().to_rfc3339(),
                    });

                    if let Ok(count_json) = serde_json::to_string(&comment_count_msg) {
                        let _ = broadcaster.send(count_json);
                    }
                }
            }

            Some("vote.update") => {
                if let Some(payload) = json_msg.get("payload") {
                    let payload_clone = payload.clone();
                    let broadcast_msg = serde_json::json!({
                        "type": "vote.update",
                        "payload": payload_clone,
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                    }
                }
            }

            Some("like") => {
                if let Some(payload) = json_msg.get("payload") {
                    let payload_clone = payload.clone();
                    let broadcast_msg = serde_json::json!({
                        "type": "like",
                        "payload": payload_clone,
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                    }
                }
            }

            Some("typing") => {
                if let Some(payload) = json_msg.get("payload") {
                    let payload_clone = payload.clone();
                    let broadcast_msg = serde_json::json!({
                        "type": "typing",
                        "payload": payload_clone,
                        "timestamp": Utc::now().to_rfc3339(),
                    });
                    if let Ok(broadcast_json) = serde_json::to_string(&broadcast_msg) {
                        let _ = broadcaster.send(broadcast_json);
                    }
                }
            }

            Some("room.join") => {
                tracing::info!("User {} joined room for fixture {}", user_id, fixture_id);
            }

            Some("room.leave") => {
                tracing::info!("User {} left room for fixture {}", user_id, fixture_id);
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

async fn save_comment_to_database(state: &AppState, payload: &Value) -> Result<(), String> {
    let collection: mongodb::Collection<Comment> = state.db.collection("room");

    let from_user_id = payload
        .get("fromUserId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let username = payload
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let fixture_id = payload
        .get("fixtureId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let message = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let selection = payload
        .get("selection")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let message_id = payload
        .get("messageId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !message_id.is_empty() {
        let existing = collection
            .find_one(doc! { "messageId": &message_id })
            .await
            .unwrap_or(None);

        if existing.is_some() {
            tracing::warn!(
                "⚠️ Duplicate messageId detected, skipping save: {}",
                message_id
            );
            return Ok(());
        }
    }

    let reply_to = if let Some(reply_json) = payload.get("replyTo") {
        match serde_json::from_value::<ReplyData>(reply_json.clone()) {
            Ok(reply) => Some(reply),
            Err(e) => {
                tracing::warn!("Failed to parse replyTo: {}", e);
                None
            }
        }
    } else {
        None
    };

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

    let now = Utc::now();

    let comment = Comment {
        id: None,
        voter_id: from_user_id,
        username,
        fixture_id: fixture_id.clone(),
        selection,
        comment: message,
        timestamp: now.to_rfc3339(),
        comment_timestamp: BsonDateTime::from_millis(now.timestamp_millis()),
        created_at: Some(BsonDateTime::from_millis(now.timestamp_millis())),
        likes: Some(0),
        replies: Some(Vec::new()),
        seen_by: vec![],
        image_url,
        video_url,
        is_image,
        is_video,
        reply_to,
        message_id: if message_id.is_empty() {
            None
        } else {
            Some(message_id.clone())
        },
    };

    match collection.insert_one(comment).await {
        Ok(_) => {
            tracing::info!("✅ Comment saved with messageId: {}", message_id);

            if !fixture_id.is_empty() {
                let games_collection = state.db.collection::<Game>("games");
                let game_filter = doc! { "match_id": &fixture_id };
                let update = doc! { "$inc": { "comments": 1 } };

                match games_collection.update_one(game_filter, update).await {
                    Ok(result) => {
                        tracing::info!(
                            "✅ games.comments +1 for fixture {} (modified: {})",
                            fixture_id,
                            result.modified_count
                        );
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to update games collection: {}", e);
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            tracing::error!("❌ Failed to save comment: {}", e);
            Err(format!("Database error: {}", e))
        }
    }
}

async fn delete_comment_from_database(state: &AppState, message_id: &str) -> Result<(), String> {
    let collection: mongodb::Collection<Comment> = state.db.collection("room");

    let by_message_id_filter = doc! { "messageId": message_id };
    let comment_by_msg_id = collection
        .find_one(by_message_id_filter.clone())
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    if let Some(comment) = comment_by_msg_id {
        let fixture_id = comment.fixture_id.clone();

        let result = collection
            .delete_one(by_message_id_filter)
            .await
            .map_err(|e| format!("Delete failed: {}", e))?;

        if result.deleted_count > 0 {
            tracing::info!("✅ Deleted by messageId field: {}", message_id);
            decrement_game_comment_count(state, &fixture_id).await;
            return Ok(());
        }
    }

    if let Ok(oid) = ObjectId::parse_str(message_id) {
        let by_oid_filter = doc! { "_id": oid };
        let comment_by_oid = collection
            .find_one(by_oid_filter.clone())
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        if let Some(comment) = comment_by_oid {
            let fixture_id = comment.fixture_id.clone();

            let result = collection
                .delete_one(by_oid_filter)
                .await
                .map_err(|e| format!("Delete failed: {}", e))?;

            if result.deleted_count > 0 {
                tracing::info!("✅ Deleted by _id: {}", message_id);
                decrement_game_comment_count(state, &fixture_id).await;
                return Ok(());
            }
        }
    }

    let error_msg = format!("❌ Comment not found for deletion: {}", message_id);
    tracing::error!("{}", error_msg);
    Err(error_msg)
}

async fn decrement_game_comment_count(state: &AppState, fixture_id: &str) {
    let games_collection = state.db.collection::<Game>("games");
    let game_filter = doc! { "match_id": fixture_id };
    let update = doc! { "$inc": { "comments": -1 } };

    match games_collection.update_one(game_filter, update).await {
        Ok(result) => {
            tracing::info!(
                "✅ games.comments -1 for fixture {} (modified: {})",
                fixture_id,
                result.modified_count
            );
        }
        Err(e) => {
            tracing::error!("❌ Failed to decrement games.comments: {}", e);
        }
    }
}

async fn send_current_match_state(
    state: &AppState,
    fixture_id: &str,
    sender: &Arc<Mutex<futures_util::stream::SplitSink<WebSocket, Message>>>,
) {
    let collection = state.db.collection::<Game>("games");
    let filter = doc! { "match_id": fixture_id };

    if let Ok(Some(game)) = collection.find_one(filter).await {
        let score_msg = serde_json::json!({
            "type": "match.score",
            "payload": {
                "fixture_id": fixture_id,
                "home_score": game.home_score.unwrap_or(0),
                "away_score": game.away_score.unwrap_or(0),
                "minute": game.time_elapsed,
            },
            "timestamp": Utc::now().to_rfc3339(),
        });

        if let Ok(score_json) = serde_json::to_string(&score_msg) {
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(score_json)).await;
        }

        let status_msg = serde_json::json!({
            "type": "match.status",
            "payload": {
                "fixture_id": fixture_id,
                "status": game.status,
                "time_elapsed": game.time_elapsed,
            },
            "timestamp": Utc::now().to_rfc3339(),
        });

        if let Ok(status_json) = serde_json::to_string(&status_msg) {
            let mut sender_guard = sender.lock().await;
            let _ = sender_guard.send(Message::Text(status_json)).await;
        }
    }
}

async fn get_comment_count(state: &AppState, fixture_id: &str) -> i64 {
    let games_collection = state.db.collection::<Game>("games");
    let filter = doc! { "match_id": fixture_id };

    match games_collection.find_one(filter).await {
        Ok(Some(game)) => {
            let count = game.comments;
            tracing::info!(
                "📊 Comment count from games collection for {}: {}",
                fixture_id,
                count
            );
            count
        }
        Ok(None) => {
            tracing::warn!("⚠️ Game not found for fixture: {}", fixture_id);
            0
        }
        Err(e) => {
            tracing::error!("❌ Failed to get comment count from games: {}", e);
            0
        }
    }
}

pub async fn broadcast_live_match_update(
    state: &AppState,
    fixture_id: &str,
    event_type: &str,
    data: serde_json::Value,
) {
    let ws_message = match event_type {
        "goal" => {
            if let Ok(payload) = serde_json::from_value::<GoalPayload>(data) {
                Some(serde_json::json!({
                    "type": "match.goal",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "score" => {
            if let Ok(payload) = serde_json::from_value::<ScorePayload>(data) {
                Some(serde_json::json!({
                    "type": "match.score",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "yellow_card" | "red_card" => {
            if let Ok(payload) = serde_json::from_value::<CardPayload>(data) {
                Some(serde_json::json!({
                    "type": "match.card",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "half_time" => {
            if let Ok(payload) = serde_json::from_value::<HalfTimePayload>(data) {
                Some(serde_json::json!({
                    "type": "match.half_time",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "full_time" => {
            if let Ok(payload) = serde_json::from_value::<FullTimePayload>(data) {
                Some(serde_json::json!({
                    "type": "match.full_time",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "statistics" => {
            if let Ok(payload) = serde_json::from_value::<StatisticsPayload>(data) {
                Some(serde_json::json!({
                    "type": "match.statistics",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "status" => {
            if let Ok(payload) = serde_json::from_value::<StatusPayload>(data) {
                Some(serde_json::json!({
                    "type": "match.status",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        "commentary" => {
            if let Ok(payload) = serde_json::from_value::<CommentaryEntry>(data) {
                Some(serde_json::json!({
                    "type": "commentary.new",
                    "payload": payload,
                    "timestamp": Utc::now().to_rfc3339(),
                }))
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(message) = ws_message {
        if let Ok(json) = serde_json::to_string(&message) {
            let tx = state.get_or_create_broadcaster(fixture_id);
            let _ = tx.send(json);
            tracing::info!(
                "📡 Broadcasted {} event for fixture {}",
                event_type,
                fixture_id
            );
        }
    }
}
