// File: src/handlers/comment_handlers.rs

use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId};
use mongodb::{options::FindOptions, Collection};
use serde_json::json;
use uuid::Uuid;

use crate::errors::{AppError, Result};
use crate::models::comments_model::{
    Comment, CommentResponse, CreateCommentRequest, UpdateCommentRequest,
    LikeCommentRequest, GetCommentsQuery,
};
use crate::models::notification::FCMToken;
use crate::models::posta::Post;
use crate::state::AppState;

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

// Helper function to get all user IDs from fcm_tokens collection
async fn get_all_user_ids(state: &AppState, exclude_user_id: Option<&str>) -> Vec<String> {
    let tokens_collection: Collection<FCMToken> = state.db.collection("fcm_tokens");

    match tokens_collection.distinct("user_id", doc! {}).await {
        Ok(user_ids_bson) => {
            let mut ids = Vec::new();
            for user_id_bson in user_ids_bson {
                if let Some(user_id_str) = user_id_bson.as_str() {
                    if let Some(exclude) = exclude_user_id {
                        if user_id_str != exclude {
                            ids.push(user_id_str.to_string());
                        }
                    } else {
                        ids.push(user_id_str.to_string());
                    }
                }
            }
            println!("📱 Found {} total users to notify", ids.len());
            ids
        }
        Err(e) => {
            eprintln!("❌ Error getting user IDs: {}", e);
            Vec::new()
        }
    }
}

macro_rules! log_info {
    ($($arg:tt)*) => {
        println!("[INFO] [{}] {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), format!($($arg)*))
    }
}

macro_rules! log_error {
    ($($arg:tt)*) => {
        eprintln!("[ERROR] [{}] {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), format!($($arg)*))
    }
}

// ============================================================================
// CREATE COMMENT (WITH REPLY SUPPORT)
// ============================================================================
pub async fn create_comment(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] create_comment called for post: {}", request_id, post_id);

    // Validate comment
    if payload.comment.trim().is_empty() {
        return Err(AppError::invalid_data("Comment cannot be empty"));
    }

    if payload.user_id.trim().is_empty() || payload.user_name.trim().is_empty() {
        return Err(AppError::invalid_data("User ID and name are required"));
    }

    let comment_collection: Collection<Comment> = state.db.collection("comments");
    let post_collection: Collection<Post> = state.db.collection("posts");

    // Verify post exists
    let post_object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match post_collection
        .find_one(doc! { "_id": post_object_id })
        .await?
    {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    // Check if user already commented (only for top-level comments, not replies)
    if payload.parent_comment_id.is_none() {
        let existing_comment = comment_collection
            .find_one(doc! {
                "post_id": &post_id,
                "user_id": &payload.user_id,
                "parent_comment_id": null,
            })
            .await?;

        if existing_comment.is_some() {
            return Err(AppError::invalid_data(
                "You have already commented on this post. You can edit your existing comment.",
            ));
        }
    }

    // If this is a reply, verify parent comment exists
    if let Some(ref parent_id) = payload.parent_comment_id {
        let parent_object_id = match ObjectId::parse_str(parent_id) {
            Ok(oid) => oid,
            Err(_) => return Err(AppError::invalid_data("Invalid parent comment ID")),
        };

        let parent_exists = comment_collection
            .find_one(doc! { "_id": parent_object_id })
            .await?
            .is_some();

        if !parent_exists {
            return Err(AppError::invalid_data("Parent comment not found"));
        }
    }

    // Create comment with parent_comment_id support
    let mut comment = Comment::new(
        post_id.clone(),
        payload.user_id.clone(),
        payload.user_name.clone(),
        payload.comment.clone(),
        payload.parent_comment_id.clone(),
    );

    // Set timestamp
    comment.timestamp = Utc::now().timestamp();

    let insert_result = comment_collection.insert_one(&comment).await?;

    if let Some(comment_id) = insert_result.inserted_id.as_object_id() {
        let comment_id_hex = comment_id.to_hex();

        // Increment post comments count
        let _ = post_collection
            .update_one(
                doc! { "_id": post_object_id },
                doc! {
                    "$inc": { "comments_count": 1 },
                    "$set": {
                        "updated_at": Utc::now(),
                        "last_modified": Utc::now()
                    }
                },
            )
            .await;

        // If this is a reply, increment parent comment's reply count
        if let Some(ref parent_id) = payload.parent_comment_id {
            if let Ok(parent_object_id) = ObjectId::parse_str(parent_id) {
                let _ = comment_collection
                    .update_one(
                        doc! { "_id": parent_object_id },
                        doc! {
                            "$inc": { "reply_count": 1 },
                            "$set": {
                                "updated_at": Utc::now(),
                                "last_modified": Utc::now()
                            }
                        },
                    )
                    .await;
            }
        }

        // Send notifications
        let state_clone = state.clone();
        let commenter_name = payload.user_name.clone();
        let comment_text = payload.comment.clone();
        let post_id_clone = post_id.clone();
        let parent_comment_id_clone = payload.parent_comment_id.clone();
        let comment_id_hex_clone = comment_id_hex.clone();
        let user_id_clone = payload.user_id.clone();

        tokio::spawn(async move {
            if let Some(fcm_service) = &state_clone.fcm_service {
                // Get parent comment author for reply notifications
                let mut parent_author_id: Option<String> = None;
                let mut parent_author_name: Option<String> = None;

                if let Some(ref parent_id) = parent_comment_id_clone {
                    if let Ok(parent_oid) = ObjectId::parse_str(parent_id) {
                        if let Ok(Some(parent_comment)) = comment_collection
                            .find_one(doc! { "_id": parent_oid })
                            .await
                        {
                            parent_author_id = Some(parent_comment.user_id);
                            parent_author_name = Some(parent_comment.user_name);
                        }
                    }
                }

                let all_user_ids = get_all_user_ids(&state_clone, Some(&user_id_clone)).await;

                let comment_preview = if comment_text.len() > 100 {
                    format!("{}...", &comment_text[0..100])
                } else {
                    comment_text.clone()
                };

                if !all_user_ids.is_empty() {
                    // Determine notification type
                    let (notification_type, title, body) = if parent_comment_id_clone.is_some() {
                        (
                            "comment_reply",
                            format!("💬 {} replied to a comment", commenter_name),
                            format!(
                                "Replied to @{}: {}",
                                parent_author_name.as_deref().unwrap_or("someone"),
                                comment_preview
                            ),
                        )
                    } else {
                        (
                            "post_comment",
                            format!("💬 {} commented on a post", commenter_name),
                            comment_preview.clone(),
                        )
                    };

                    let _ = fcm_service
                        .send_to_multiple_users(
                            &state_clone,
                            all_user_ids,
                            &title,
                            &body,
                            serde_json::json!({
                                "post_id": post_id_clone,
                                "comment_id": comment_id_hex_clone,
                                "commenter_id": user_id_clone,
                                "commenter_name": commenter_name,
                                "comment_preview": &comment_preview,
                                "parent_comment_id": parent_comment_id_clone,
                                "parent_author_id": parent_author_id,
                                "is_reply": parent_comment_id_clone.is_some(),
                                "type": notification_type,
                                "timestamp": Utc::now().to_rfc3339(),
                            }),
                            notification_type,
                        )
                        .await;
                }

                // Send specific notification to parent comment author if reply
                if let Some(ref parent_author_id) = parent_author_id {
                    if parent_author_id != &user_id_clone {
                        let _ = fcm_service
                            .send_to_user(
                                &state_clone,
                                parent_author_id,
                                &format!("💬 {} replied to your comment", commenter_name),
                                &comment_preview,
                                serde_json::json!({
                                    "post_id": post_id_clone,
                                    "comment_id": comment_id_hex_clone,
                                    "commenter_id": user_id_clone,
                                    "commenter_name": commenter_name,
                                    "comment_preview": &comment_preview,
                                    "parent_comment_id": parent_comment_id_clone,
                                    "type": "comment_reply",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "comment_reply",
                            )
                            .await;
                    }
                }

                // Send notification to post owner if different
                if post.user_id != user_id_clone {
                    let post_owner_id = post.user_id.clone();
                    if Some(&post_owner_id) != parent_author_id.as_ref() {
                        let _ = fcm_service
                            .send_to_user(
                                &state_clone,
                                &post_owner_id,
                                &format!("💬 {} commented on your post", commenter_name),
                                &comment_preview,
                                serde_json::json!({
                                    "post_id": post_id_clone,
                                    "comment_id": comment_id_hex_clone,
                                    "commenter_id": user_id_clone,
                                    "commenter_name": commenter_name,
                                    "comment_preview": &comment_preview,
                                    "is_reply": parent_comment_id_clone.is_some(),
                                    "type": "post_comment",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "post_comment",
                            )
                            .await;
                    }
                }
            }
        });

        let comment_response = CommentResponse::from(comment);

        log_info!("[{}] Comment created successfully: {}", request_id, comment_id_hex);
        Ok(Json(json!({
            "success": true,
            "message": "Comment created successfully",
            "comment": comment_response
        })))
    } else {
        Err(AppError::service("Failed to create comment"))
    }
}

// ============================================================================
// GET COMMENTS (WITH PAGINATION AND REPLIES)
// ============================================================================
// ============================================================================
// GET COMMENTS (WITH PAGINATION AND REPLIES)
// ============================================================================
pub async fn get_comments(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Query(params): Query<GetCommentsQuery>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] get_comments called for post: {}", request_id, post_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    // Build filter — field names match the actual BSON keys stored via
    // #[serde(rename = "...")] on Comment (postId, userId,
    // parentCommentId), NOT the Rust snake_case field names.
    let mut filter = doc! { "postId": &post_id };

    // If exclude_replies is true, only get top-level comments
    if params.exclude_replies {
        filter.insert("parentCommentId", doc! { "$eq": null });
    }

    // Filter by user if provided
    if let Some(user_id) = &params.user_id {
        filter.insert("userId", user_id);
    }

    // Pagination
    let page = params.page.max(1);
    let limit = params.limit.min(MAX_PAGE_SIZE).max(1);
    let skip = (page - 1) * limit;

    // Sort — same rule: use the stored camelCase field names
    let sort_order = match params.sort.as_str() {
        "oldest" => doc! { "createdAt": 1 },
        "likes" => doc! { "likesCount": -1, "createdAt": -1 },
        "replies" => doc! { "replyCount": -1, "createdAt": -1 },
        _ => doc! { "createdAt": -1 }, // newest default
    };

    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = if total_count > 0 {
        (total_count as f64 / limit as f64).ceil() as i64
    } else {
        0
    };

    let options = FindOptions::builder()
        .sort(sort_order)
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection
        .find(filter)
        .with_options(options)
        .await?;

    let comments: Vec<Comment> = cursor.try_collect().await?;

    // Get replies for each comment if not excluding replies
    let mut comment_responses: Vec<CommentResponse> = Vec::new();

    for comment in comments {
        let mut comment_response = CommentResponse::from(comment.clone());

        // If this is a top-level comment and we're not excluding replies, fetch its replies
        if !params.exclude_replies && comment.parent_comment_id.is_none() && comment.reply_count > 0 {
            if let Some(ref comment_id) = comment.id {
                let comment_id_hex = comment_id.to_hex();
                let replies_cursor = collection
                    .find(doc! { "parentCommentId": &comment_id_hex })
                    .with_options(
                        FindOptions::builder()
                            .sort(doc! { "createdAt": 1 })
                            .limit(10) // Limit replies to 10 per comment
                            .build()
                    )
                    .await?;

                let replies: Vec<Comment> = replies_cursor.try_collect().await?;
                let reply_responses: Vec<CommentResponse> = replies.into_iter()
                    .map(CommentResponse::from)
                    .collect();

                comment_response.replies = Some(reply_responses);
            }
        }

        comment_responses.push(comment_response);
    }

    log_info!("[{}] get_comments completed. Found {} comments", request_id, comment_responses.len());

    Ok(Json(json!({
        "success": true,
        "comments": comment_responses,
        "post_id": post_id,
        "pagination": {
            "page": page,
            "limit": limit,
            "total_count": total_count,
            "total_pages": total_pages,
            "has_next": page < total_pages,
            "has_previous": page > 1
        }
    })))
}
// ============================================================================
// GET COMMENT BY ID
// ============================================================================
pub async fn get_comment_by_id(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] get_comment_by_id called for comment: {}", request_id, comment_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

    let comment_response = CommentResponse::from(comment);

    Ok(Json(json!({
        "success": true,
        "comment": comment_response
    })))
}

// ============================================================================
// UPDATE COMMENT
// ============================================================================
pub async fn update_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] update_comment called for comment: {}", request_id, comment_id);

    if payload.comment.trim().is_empty() {
        return Err(AppError::invalid_data("Comment cannot be empty"));
    }

    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

    // Verify ownership
    if comment.user_id != payload.user_id {
        return Err(AppError::invalid_data(
            "You can only edit your own comments",
        ));
    }

    let filter = doc! { "_id": object_id };
    let update = doc! {
        "$set": {
            "comment": payload.comment.clone(),
            "updated_at": Utc::now(),
            "last_modified": Utc::now()
        }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::invalid_data("Comment not found"));
    }

    match collection.find_one(doc! { "_id": object_id }).await? {
        Some(updated_comment) => {
            let comment_response = CommentResponse::from(updated_comment);
            log_info!("[{}] Comment updated successfully: {}", request_id, comment_id);
            Ok(Json(json!({
                "success": true,
                "message": "Comment updated successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ============================================================================
// DELETE COMMENT
// ============================================================================
pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] delete_comment called for comment: {}", request_id, comment_id);

    let comment_collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match comment_collection
        .find_one(doc! { "_id": object_id })
        .await?
    {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

    // Verify ownership
    if comment.user_id != payload.user_id {
        return Err(AppError::invalid_data(
            "You can only delete your own comments",
        ));
    }

    // If this comment has replies, we need to handle them
    if comment.reply_count > 0 {
        // Option 1: Delete all replies too
        let _ = comment_collection
            .delete_many(doc! { "parent_comment_id": &comment_id })
            .await;

        // Option 2: Or move replies to top-level (uncomment to use)
        // let _ = comment_collection
        //     .update_many(
        //         doc! { "parent_comment_id": &comment_id },
        //         doc! { "$set": { "parent_comment_id": null } }
        //     )
        //     .await;
    }

    let result = comment_collection
        .delete_one(doc! { "_id": object_id })
        .await?;

    if result.deleted_count > 0 {
        // Decrement post comments count
        let post_object_id = ObjectId::parse_str(&comment.post_id);
        if let Ok(post_id) = post_object_id {
            let post_collection: Collection<Post> = state.db.collection("posts");
            let _ = post_collection
                .update_one(
                    doc! { "_id": post_id },
                    doc! {
                        "$inc": { "comments_count": -1 },
                        "$set": {
                            "updated_at": Utc::now(),
                            "last_modified": Utc::now()
                        }
                    },
                )
                .await;
        }

        // If this was a reply, decrement parent's reply count
        if let Some(parent_id) = &comment.parent_comment_id {
            if let Ok(parent_object_id) = ObjectId::parse_str(parent_id) {
                let _ = comment_collection
                    .update_one(
                        doc! { "_id": parent_object_id },
                        doc! {
                            "$inc": { "reply_count": -1 },
                            "$set": {
                                "updated_at": Utc::now(),
                                "last_modified": Utc::now()
                            }
                        },
                    )
                    .await;
            }
        }

        log_info!("[{}] Comment deleted successfully: {}", request_id, comment_id);
        Ok(Json(json!({
            "success": true,
            "message": "Comment deleted successfully",
            "comment_id": comment_id
        })))
    } else {
        Err(AppError::invalid_data("Comment not found"))
    }
}

// ============================================================================
// LIKE COMMENT
// ============================================================================
pub async fn like_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] like_comment called for comment: {}", request_id, comment_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

    // Check if already liked
    if comment.liked_by.contains(&payload.user_id) {
        let comment_response = CommentResponse::from(comment);
        return Ok(Json(json!({
            "success": true,
            "message": "Comment already liked by user",
            "comment": comment_response
        })));
    }

    let filter = doc! { "_id": object_id };
    let update = doc! {
        "$inc": { "likes_count": 1 },
        "$push": { "liked_by": &payload.user_id },
        "$set": {
            "updated_at": Utc::now(),
            "last_modified": Utc::now()
        }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::invalid_data("Comment not found"));
    }

    match collection.find_one(doc! { "_id": object_id }).await? {
        Some(updated_comment) => {
            // Send notification
            let state_clone = state.clone();
            let comment_owner_id = updated_comment.user_id.clone();
            let liker_name = payload.user_name.clone();
            let comment_id_clone = comment_id.clone();
            let likes_count = updated_comment.likes_count;

            tokio::spawn(async move {
                if let Some(fcm_service) = &state_clone.fcm_service {
                    if comment_owner_id != payload.user_id {
                        let _ = fcm_service
                            .send_to_user(
                                &state_clone,
                                &comment_owner_id,
                                &format!("❤️ {} liked your comment", liker_name),
                                &format!("Now {} people like this comment", likes_count),
                                serde_json::json!({
                                    "comment_id": comment_id_clone,
                                    "liker_id": payload.user_id,
                                    "liker_name": liker_name,
                                    "likes_count": likes_count,
                                    "type": "comment_like",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "comment_like",
                            )
                            .await;
                    }
                }
            });

            let comment_response = CommentResponse::from(updated_comment);
            log_info!("[{}] Comment liked successfully: {}", request_id, comment_id);
            Ok(Json(json!({
                "success": true,
                "message": "Comment liked successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ============================================================================
// UNLIKE COMMENT
// ============================================================================
pub async fn unlike_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] unlike_comment called for comment: {}", request_id, comment_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

    // Check if not liked
    if !comment.liked_by.contains(&payload.user_id) {
        let comment_response = CommentResponse::from(comment);
        return Ok(Json(json!({
            "success": true,
            "message": "Comment not liked by user",
            "comment": comment_response
        })));
    }

    let filter = doc! { "_id": object_id };
    let update = doc! {
        "$inc": { "likes_count": -1 },
        "$pull": { "liked_by": &payload.user_id },
        "$set": {
            "updated_at": Utc::now(),
            "last_modified": Utc::now()
        }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::invalid_data("Comment not found"));
    }

    match collection.find_one(doc! { "_id": object_id }).await? {
        Some(updated_comment) => {
            let comment_response = CommentResponse::from(updated_comment);
            log_info!("[{}] Comment unliked successfully: {}", request_id, comment_id);
            Ok(Json(json!({
                "success": true,
                "message": "Comment unliked successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ============================================================================
// GET COMMENT COUNT FOR POST
// ============================================================================
pub async fn get_comment_count(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] get_comment_count called for post: {}", request_id, post_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    let count = collection
        .count_documents(doc! { "post_id": &post_id })
        .await?;

    Ok(Json(json!({
        "success": true,
        "post_id": post_id,
        "count": count
    })))
}

// ============================================================================
// GET COMMENT REPLIES
// ============================================================================
pub async fn get_comment_replies(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Query(params): Query<GetCommentsQuery>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] get_comment_replies called for comment: {}", request_id, comment_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    // Verify parent comment exists
    let parent_object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let parent_exists = collection
        .find_one(doc! { "_id": parent_object_id })
        .await?
        .is_some();

    if !parent_exists {
        return Err(AppError::invalid_data("Parent comment not found"));
    }

    // Pagination (see note in get_comments — these fields are plain i64)
    let page = params.page.max(1);
    let limit = params.limit.min(MAX_PAGE_SIZE).max(1);
    let skip = (page - 1) * limit;

    let filter = doc! { "parent_comment_id": &comment_id };

    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = if total_count > 0 {
        (total_count as f64 / limit as f64).ceil() as i64
    } else {
        0
    };

    let options = FindOptions::builder()
        .sort(doc! { "created_at": 1 }) // Oldest first for replies
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection
        .find(filter)
        .with_options(options)
        .await?;

    let replies: Vec<Comment> = cursor.try_collect().await?;
    let reply_responses: Vec<CommentResponse> = replies.into_iter()
        .map(CommentResponse::from)
        .collect();

    log_info!("[{}] get_comment_replies completed. Found {} replies", request_id, reply_responses.len());

    Ok(Json(json!({
        "success": true,
        "comment_id": comment_id,
        "replies": reply_responses,
        "pagination": {
            "page": page,
            "limit": limit,
            "total_count": total_count,
            "total_pages": total_pages,
            "has_next": page < total_pages,
            "has_previous": page > 1
        }
    })))
}

// ============================================================================
// GET USER COMMENTS
// ============================================================================
pub async fn get_user_comments(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<GetCommentsQuery>,
) -> Result<Json<serde_json::Value>> {
    let request_id = Uuid::new_v4();
    log_info!("[{}] get_user_comments called for user: {}", request_id, user_id);

    let collection: Collection<Comment> = state.db.collection("comments");

    let mut filter = doc! { "user_id": &user_id };

    // Optionally filter by post
    if let Some(post_id) = &params.post_id {
        filter.insert("post_id", post_id);
    }

    // Pagination (see note in get_comments — these fields are plain i64)
    let page = params.page.max(1);
    let limit = params.limit.min(MAX_PAGE_SIZE).max(1);
    let skip = (page - 1) * limit;

    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = if total_count > 0 {
        (total_count as f64 / limit as f64).ceil() as i64
    } else {
        0
    };

    let options = FindOptions::builder()
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection
        .find(filter)
        .with_options(options)
        .await?;

    let comments: Vec<Comment> = cursor.try_collect().await?;
    let comment_responses: Vec<CommentResponse> = comments.into_iter()
        .map(CommentResponse::from)
        .collect();

    log_info!("[{}] get_user_comments completed. Found {} comments", request_id, comment_responses.len());

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "comments": comment_responses,
        "pagination": {
            "page": page,
            "limit": limit,
            "total_count": total_count,
            "total_pages": total_pages,
            "has_next": page < total_pages,
            "has_previous": page > 1
        }
    })))
}