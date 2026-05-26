use crate::models::notification::{FCMToken, Notification};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use bson::{doc, oid::ObjectId};
use chrono::Utc;
use futures::TryStreamExt;
use mongodb::{Collection, Database};

use crate::models::chat::{
    ApiResponse, ChatMessage, ChatMessageResponse, CreateChatMessage, MarkAsSeenRequest,
    PaginationQuery, UpdateChatMessage,
};
use crate::state::AppState;

// Get collection helper
fn get_chat_collection(db: &Database) -> Collection<ChatMessage> {
    db.collection("chat_messages")
}

// GET /chat/:post_id/messages
pub async fn get_post_messages(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> impl IntoResponse {
    println!("📥 Fetching messages for post: {}", post_id);

    let collection = get_chat_collection(&state.db);

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(50);
    let skip = (page - 1) * limit;

    let filter = doc! { "postId": &post_id };

    match collection.count_documents(filter.clone()).await {
        Ok(total) => {
            println!("📊 Total messages: {}", total);

            let cursor = collection
                .find(filter)
                .sort(doc! { "createdAt": 1 })
                .skip(skip as u64)
                .limit(limit as i64)
                .await;

            match cursor {
                Ok(mut cursor) => {
                    let mut messages = Vec::new();
                    while let Ok(Some(message)) = cursor.try_next().await {
                        messages.push(ChatMessageResponse::from(message));
                    }

                    println!("✅ Found {} messages", messages.len());

                    let response = serde_json::json!({
                        "messages": messages,
                        "total": total,
                        "page": page,
                        "limit": limit,
                        "totalPages": (total as f64 / limit as f64).ceil() as i64
                    });

                    (StatusCode::OK, Json(ApiResponse::success(response)))
                }
                Err(err) => {
                    eprintln!("❌ Error fetching messages: {}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error(format!(
                            "Failed to fetch messages: {}",
                            err
                        ))),
                    )
                }
            }
        }
        Err(err) => {
            eprintln!("❌ Error counting messages: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!(
                    "Failed to count messages: {}",
                    err
                ))),
            )
        }
    }
}

// POST /chat/:post_id/messages
// POST /chat/:post_id/messages - UPDATED WITH FCM NOTIFICATIONS
pub async fn create_message(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<CreateChatMessage>,
) -> impl IntoResponse {
    println!("📝 Creating message for post: {}", post_id);
    println!("📨 Sender: {} ({})", payload.sender_name, payload.sender_id);
    println!(
        "📨 Receiver: {} ({})",
        payload.receiver_name, payload.receiver_id
    );
    println!("💬 Message: {}", payload.message);

    let collection = get_chat_collection(&state.db);

    // Create new message document
    let chat_message = ChatMessage {
        id: None,
        post_id: post_id.clone(),
        sender_id: payload.sender_id.clone(),
        receiver_id: payload.receiver_id.clone(),
        sender_name: payload.sender_name.clone(),
        receiver_name: payload.receiver_name.clone(),
        message: payload.message.clone(),
        seen: false,
        created_at: bson::DateTime::from_chrono(Utc::now()),
        updated_at: None,
    };

    println!("💾 Saving message to database...");

    match collection.insert_one(&chat_message).await {
        Ok(insert_result) => {
            println!(
                "✅ Message inserted with ID: {:?}",
                insert_result.inserted_id
            );

            if let Some(object_id) = insert_result.inserted_id.as_object_id() {
                let filter = doc! { "_id": object_id };
                match collection.find_one(filter).await {
                    Ok(Some(saved_message)) => {
                        let response = ChatMessageResponse::from(saved_message.clone());
                        println!("✅ Message saved and retrieved successfully");

                        // ========== SEND FCM NOTIFICATION TO RECEIVER ==========
                        let state_clone = state.clone();
                        let receiver_id = payload.receiver_id.clone();
                        let sender_name = payload.sender_name.clone();
                        let message_text = payload.message.clone();
                        let message_id = object_id.to_hex();
                        let post_id_clone = post_id.clone();

                        tokio::spawn(async move {
                            println!("📱 Sending push notification to receiver: {}", receiver_id);

                            // Initialize FCM service
                            if let Ok(fcm_service) =
                                crate::services::fcm_service::init_fcm_service().await
                            {
                                // Get receiver's FCM tokens
                                let tokens_collection: Collection<FCMToken> =
                                    state_clone.db.collection("fcm_tokens");
                                let token_filter = doc! { "user_id": &receiver_id };

                                match tokens_collection.count_documents(token_filter).await {
                                    Ok(token_count) => {
                                        if token_count > 0 {
                                            println!(
                                                "📱 Found {} tokens for receiver",
                                                token_count
                                            );

                                            // Create message preview
                                            let message_preview = if message_text.len() > 50 {
                                                format!("{}...", &message_text[0..50])
                                            } else {
                                                message_text.clone()
                                            };

                                            // Send notification
                                            let notification_result = fcm_service
                                                .send_to_user(
                                                    &state_clone,
                                                    &receiver_id,
                                                    &format!("💬 New message from {}", sender_name),
                                                    &message_preview,
                                                    serde_json::json!({
                                                        "post_id": post_id_clone,
                                                        "message_id": message_id,
                                                        "sender_id": payload.sender_id,
                                                        "sender_name": sender_name,
                                                        "receiver_id": receiver_id,
                                                        "message_preview": message_preview,
                                                        "type": "chat_message",
                                                        "timestamp": Utc::now().to_rfc3339(),
                                                    }),
                                                    "chat_message",
                                                )
                                                .await;

                                            match notification_result {
                                                Ok(sent) => {
                                                    if sent {
                                                        println!("✅ Push notification sent successfully to receiver");
                                                    } else {
                                                        println!("⚠️ Push notification may not have been delivered (no tokens?)");
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!(
                                                        "❌ Error sending push notification: {}",
                                                        e
                                                    );
                                                }
                                            }

                                            // Save notification to database
                                            let notifications_collection: Collection<Notification> =
                                                state_clone.db.collection("notifications");

                                            let notification = Notification {
                                                id: None,
                                                user_id: receiver_id,
                                                notification_type: "chat_message".to_string(),
                                                title: format!("New message from {}", sender_name),
                                                body: message_preview,
                                                data: serde_json::json!({
                                                    "post_id": post_id_clone,
                                                    "message_id": message_id,
                                                    "sender_id": payload.sender_id,
                                                    "sender_name": sender_name,
                                                    "type": "chat_message"
                                                }),
                                                is_read: false,
                                                created_at: bson::DateTime::from_chrono(Utc::now()),
                                            };

                                            let _ = notifications_collection
                                                .insert_one(notification)
                                                .await;
                                        } else {
                                            println!(
                                                "📱 No FCM tokens found for receiver: {}",
                                                receiver_id
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Error checking receiver tokens: {}", e);
                                    }
                                }
                            } else {
                                eprintln!("❌ Failed to initialize FCM service");
                            }
                        });
                        // ========== END FCM NOTIFICATION ==========

                        (StatusCode::CREATED, Json(ApiResponse::success(response)))
                    }
                    Ok(None) => {
                        eprintln!("❌ Message was inserted but could not be retrieved");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error(
                                "Message saved but could not be retrieved".to_string(),
                            )),
                        )
                    }
                    Err(err) => {
                        eprintln!("❌ Error retrieving saved message: {}", err);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiResponse::error(format!(
                                "Failed to retrieve saved message: {}",
                                err
                            ))),
                        )
                    }
                }
            } else {
                eprintln!("❌ Could not extract inserted ID");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error("Failed to get message ID".to_string())),
                )
            }
        }
        Err(err) => {
            eprintln!("❌ Failed to insert message: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!(
                    "Failed to create message: {}",
                    err
                ))),
            )
        }
    }
}

// GET /chat/messages/:message_id
pub async fn get_message(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> impl IntoResponse {
    println!("🔍 Fetching message by ID: {}", message_id);

    let collection = get_chat_collection(&state.db);

    match ObjectId::parse_str(&message_id) {
        Ok(object_id) => {
            let filter = doc! { "_id": object_id };

            match collection.find_one(filter).await {
                Ok(Some(message)) => {
                    println!("✅ Found message: {}", message_id);
                    let response = ChatMessageResponse::from(message);
                    (StatusCode::OK, Json(ApiResponse::success(response)))
                }
                Ok(None) => {
                    println!("❌ Message not found: {}", message_id);
                    (
                        StatusCode::NOT_FOUND,
                        Json(ApiResponse::error("Message not found".to_string())),
                    )
                }
                Err(err) => {
                    eprintln!("❌ Error fetching message: {}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error(format!(
                            "Failed to fetch message: {}",
                            err
                        ))),
                    )
                }
            }
        }
        Err(_) => {
            eprintln!("❌ Invalid message ID format: {}", message_id);
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Invalid message ID format".to_string())),
            )
        }
    }
}

// PUT /chat/messages/:message_id
pub async fn update_message(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
    Json(payload): Json<UpdateChatMessage>,
) -> impl IntoResponse {
    println!("📝 Updating message: {}", message_id);

    let collection = get_chat_collection(&state.db);

    match ObjectId::parse_str(&message_id) {
        Ok(object_id) => {
            let filter = doc! { "_id": object_id };

            let update = doc! {
                "$set": {
                    "message": &payload.message,
                    "updatedAt": bson::DateTime::from_chrono(Utc::now())
                }
            };

            match collection.find_one_and_update(filter, update).await {
                Ok(Some(updated_message)) => {
                    println!("✅ Message updated successfully: {}", message_id);
                    let response = ChatMessageResponse::from(updated_message);
                    (StatusCode::OK, Json(ApiResponse::success(response)))
                }
                Ok(None) => {
                    println!("❌ Message not found for update: {}", message_id);
                    (
                        StatusCode::NOT_FOUND,
                        Json(ApiResponse::error("Message not found".to_string())),
                    )
                }
                Err(err) => {
                    eprintln!("❌ Error updating message: {}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error(format!(
                            "Failed to update message: {}",
                            err
                        ))),
                    )
                }
            }
        }
        Err(_) => {
            eprintln!("❌ Invalid message ID format: {}", message_id);
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Invalid message ID format".to_string())),
            )
        }
    }
}

// DELETE /chat/messages/:message_id
pub async fn delete_message(
    State(state): State<AppState>,
    Path(message_id): Path<String>,
) -> impl IntoResponse {
    println!("🗑️ Deleting message: {}", message_id);

    let collection = get_chat_collection(&state.db);

    match ObjectId::parse_str(&message_id) {
        Ok(object_id) => {
            let filter = doc! { "_id": object_id };

            match collection.delete_one(filter).await {
                Ok(delete_result) if delete_result.deleted_count > 0 => {
                    println!("✅ Message deleted successfully: {}", message_id);
                    (
                        StatusCode::OK,
                        Json(ApiResponse::success(
                            "Message deleted successfully".to_string(),
                        )),
                    )
                }
                Ok(_) => {
                    println!("❌ Message not found for deletion: {}", message_id);
                    (
                        StatusCode::NOT_FOUND,
                        Json(ApiResponse::error("Message not found".to_string())),
                    )
                }
                Err(err) => {
                    eprintln!("❌ Error deleting message: {}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error(format!(
                            "Failed to delete message: {}",
                            err
                        ))),
                    )
                }
            }
        }
        Err(_) => {
            eprintln!("❌ Invalid message ID format: {}", message_id);
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Invalid message ID format".to_string())),
            )
        }
    }
}

// POST /chat/messages/mark-seen
pub async fn mark_messages_as_seen(
    State(state): State<AppState>,
    Json(payload): Json<MarkAsSeenRequest>,
) -> impl IntoResponse {
    println!("👁️ Marking messages as seen for post: {}", payload.post_id);

    let collection = get_chat_collection(&state.db);

    let filter = doc! {
        "postId": &payload.post_id,
        "receiverId": &payload.user_id,
        "seen": false
    };

    let update = doc! {
        "$set": {
            "seen": true
        }
    };

    match collection.update_many(filter, update).await {
        Ok(update_result) => {
            println!(
                "✅ Marked {} messages as seen",
                update_result.modified_count
            );

            let response = serde_json::json!({
                "marked_count": update_result.modified_count
            });

            (StatusCode::OK, Json(ApiResponse::success(response)))
        }
        Err(err) => {
            eprintln!("❌ Error marking messages as seen: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!(
                    "Failed to mark messages: {}",
                    err
                ))),
            )
        }
    }
}

// GET /chat/:post_id/unread-count
pub async fn get_unread_count(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> impl IntoResponse {
    println!("🔢 Getting unread count for post: {}", post_id);

    let collection = get_chat_collection(&state.db);

    let filter = doc! {
        "postId": &post_id,
        "seen": false
    };

    match collection.count_documents(filter).await {
        Ok(count) => {
            println!("✅ Unread messages for post {}: {}", post_id, count);

            let response = serde_json::json!({
                "post_id": post_id,
                "unread_count": count
            });

            (StatusCode::OK, Json(ApiResponse::success(response)))
        }
        Err(err) => {
            eprintln!("❌ Error counting unread messages: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!(
                    "Failed to count unread messages: {}",
                    err
                ))),
            )
        }
    }
}

// GET /chat/users/:user_id/messages
pub async fn get_user_messages(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> impl IntoResponse {
    println!("🔍 Fetching messages for user: {}", user_id);

    let collection = get_chat_collection(&state.db);

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(50);
    let skip = (page - 1) * limit;

    let filter = doc! {
        "$or": [
            { "senderId": &user_id },
            { "receiverId": &user_id }
        ]
    };

    match collection.count_documents(filter.clone()).await {
        Ok(total) => {
            println!("📊 Total messages for user {}: {}", user_id, total);

            let cursor = collection
                .find(filter)
                .sort(doc! { "createdAt": -1 })
                .skip(skip as u64)
                .limit(limit as i64)
                .await;

            match cursor {
                Ok(mut cursor) => {
                    let mut messages = Vec::new();
                    while let Ok(Some(message)) = cursor.try_next().await {
                        messages.push(ChatMessageResponse::from(message));
                    }

                    println!("✅ Found {} messages for user {}", messages.len(), user_id);

                    let response = serde_json::json!({
                        "messages": messages,
                        "total": total,
                        "page": page,
                        "limit": limit
                    });

                    (StatusCode::OK, Json(ApiResponse::success(response)))
                }
                Err(err) => {
                    eprintln!("❌ Error fetching user messages: {}", err);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiResponse::error(format!(
                            "Failed to fetch user messages: {}",
                            err
                        ))),
                    )
                }
            }
        }
        Err(err) => {
            eprintln!("❌ Error counting user messages: {}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!(
                    "Failed to count user messages: {}",
                    err
                ))),
            )
        }
    }
}

// Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    println!("❤️ Chat service health check");

    let response = serde_json::json!({
        "status": "ok",
        "service": "chat",
        "timestamp": Utc::now().to_rfc3339()
    });

    (StatusCode::OK, Json(ApiResponse::success(response)))
}
