use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Json,
};
use axum_extra::extract::Multipart;
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::bson::Bson;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::{options::FindOptions, Collection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use uuid::Uuid;

use crate::errors::{AppError, Result};
use crate::models::notification::FCMToken;
use crate::models::posta::{
    Comment, CommentResponse, CreateCommentRequest, LikeRequest, Post, PostResponse, PostType,
    UpdateCommentRequest,
};
use crate::state::AppState;

const MAX_FILE_SIZE: u64 = 40 * 1024 * 1024; // 40MB (corrected from 10MB)
const MAX_VIDEO_SIZE: u64 = 100 * 1024 * 1024; // 100MB
const ALLOWED_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "gif"];
const ALLOWED_VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "mov", "avi", "mkv", "webm"]; // Added webm
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

#[derive(Debug, Deserialize, Serialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub user_id: Option<String>,
    pub post_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateCaptionRequest {
    pub caption: String,
    pub user_id: String, // Added for ownership validation
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub user_id: Option<String>,
    pub post_type: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeletePostsByUserRequest {
    pub requesting_user_id: String, // Added for authorization
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LikeRequestWithUser {
    pub user_id: String,
    pub user_name: Option<String>,
}

fn bson_to_json_value(bson: &Bson) -> JsonValue {
    match bson {
        Bson::ObjectId(oid) => json!(oid.to_hex()),
        Bson::DateTime(dt) => json!(dt.to_chrono().to_rfc3339()),
        Bson::String(s) => json!(s),
        Bson::Int32(i) => json!(i),
        Bson::Int64(i) => json!(i),
        Bson::Double(d) => json!(d),
        Bson::Boolean(b) => json!(b),
        Bson::Null => JsonValue::Null,
        Bson::Array(arr) => JsonValue::Array(arr.iter().map(bson_to_json_value).collect()),
        Bson::Document(doc) => {
            let mut map = serde_json::Map::new();
            for (k, v) in doc {
                map.insert(k.clone(), bson_to_json_value(v));
            }
            JsonValue::Object(map)
        }
        _ => json!(bson.to_string()),
    }
}

fn document_to_json(doc: Document) -> JsonValue {
    let mut map = serde_json::Map::new();
    for (key, value) in doc {
        map.insert(key, bson_to_json_value(&value));
    }
    JsonValue::Object(map)
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

fn parse_if_modified_since(header_value: &str) -> Option<chrono::DateTime<Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(header_value) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(timestamp) = header_value.parse::<i64>() {
        return chrono::DateTime::from_timestamp(timestamp, 0);
    }

    None
}

// ========== GET POST STATS ==========
pub async fn get_post_stats(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_post_stats called");
    let collection: Collection<Post> = state.db.collection("posts");

    let total_posts = collection.count_documents(doc! {}).await?;
    let text_posts = collection
        .count_documents(doc! { "post_type": "Text" })
        .await?;
    let image_posts = collection
        .count_documents(doc! { "post_type": "Image" })
        .await?;
    let video_posts = collection
        .count_documents(doc! { "post_type": "Video" })
        .await?;
    let text_image_posts = collection
        .count_documents(doc! { "post_type": "TextAndImage" })
        .await?;
    let text_video_posts = collection
        .count_documents(doc! { "post_type": "TextAndVideo" })
        .await?;

    Ok(Json(json!({
        "success": true,
        "stats": {
            "total_posts": total_posts,
            "text_posts": text_posts,
            "image_posts": image_posts,
            "video_posts": video_posts,
            "text_image_posts": text_image_posts,
            "text_video_posts": text_video_posts,
        }
    })))
}

// ========== UPDATE POST CAPTION ==========
pub async fn update_post_caption(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<UpdateCaptionRequest>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 update_post_caption called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    // ✅ FIX: Verify ownership before allowing update
    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if post.user_id != payload.user_id {
        return Err(AppError::invalid_data(
            "You can only edit your own posts",
        ));
    }

    let result = collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "caption": payload.caption,
                    "updated_at": Utc::now(),
                    "last_modified": Utc::now()
                }
            },
        )
        .await?;

    if result.matched_count == 0 {
        return Err(AppError::PostNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Caption updated successfully"
    })))
}

// ========== GET POSTS BY USER ==========
pub async fn get_posts_by_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_posts_by_user called for user: {}", user_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let page = params.page.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(20).min(100).max(1);
    let skip = (page - 1) * limit;

    let filter = doc! { "user_id": &user_id };
    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = (total_count as f64 / limit as f64).ceil() as i64;

    let options = FindOptions::builder()
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;
    let post_responses: Vec<PostResponse> = posts.into_iter().map(PostResponse::from).collect();

    Ok(Json(json!({
        "success": true,
        "posts": post_responses,
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

// ========== GET USER POST STATS ==========
pub async fn get_user_post_stats(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_user_post_stats called for user: {}", user_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let filter = doc! { "user_id": &user_id };
    let total_posts = collection.count_documents(filter.clone()).await?;

    let text_posts = collection
        .count_documents(doc! { "user_id": &user_id, "post_type": "Text" })
        .await?;
    let image_posts = collection
        .count_documents(doc! { "user_id": &user_id, "post_type": "Image" })
        .await?;
    let video_posts = collection
        .count_documents(doc! { "user_id": &user_id, "post_type": "Video" })
        .await?;
    let text_image_posts = collection
        .count_documents(doc! { "user_id": &user_id, "post_type": "TextAndImage" })
        .await?;
    let text_video_posts = collection
        .count_documents(doc! { "user_id": &user_id, "post_type": "TextAndVideo" })
        .await?;

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "stats": {
            "total_posts": total_posts,
            "text_posts": text_posts,
            "image_posts": image_posts,
            "video_posts": video_posts,
            "text_image_posts": text_image_posts,
            "text_video_posts": text_video_posts,
        }
    })))
}

// ========== CREATE POST (PURE FIREBASE - NO CLOUDINARY) ==========
pub async fn create_post(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    println!("🔍 [{}] create_post called", request_id);

    let mut caption: Option<String> = None;
    let mut user_id = String::new();
    let mut user_name = String::new();
    let mut image_data = None;
    let mut image_extension = None;
    let mut video_data = None;
    let mut video_extension = None;
    let mut video_thumbnail_data = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        log_error!("[{}] Failed to process multipart field: {}", request_id, e);
        AppError::Multipart(format!("Failed to process multipart field: {}", e))
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        println!("🔍 [{}] Processing field: {}", request_id, field_name);

        match field_name.as_str() {
            "caption" => {
                let text = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read caption: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read caption: {}", e))
                })?;
                println!("🔍 [{}] Caption: {}", request_id, text);
                if !text.trim().is_empty() {
                    caption = Some(text);
                }
            }
            "userId" => {
                user_id = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read user_id: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read user_id: {}", e))
                })?;
                println!("🔍 [{}] User ID: {}", request_id, user_id);
            }
            "userName" => {
                user_name = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read user_name: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read user_name: {}", e))
                })?;
                println!("🔍 [{}] User Name: {}", request_id, user_name);
            }
            "image" => {
                let file_name = field.file_name().unwrap_or("image").to_string();
                let data = field.bytes().await.map_err(|e| {
                    log_error!("[{}] Failed to read image data: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read image data: {}", e))
                })?;

                println!(
                    "🔍 [{}] Image received: {}, size: {} bytes",
                    request_id,
                    file_name,
                    data.len()
                );

                if data.len() as u64 > MAX_FILE_SIZE {
                    println!(
                        "❌ [{}] Image too large: {} > {}",
                        request_id,
                        data.len(),
                        MAX_FILE_SIZE
                    );
                    return Err(AppError::ImageTooLarge);
                }

                let ext = std::path::Path::new(&file_name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                    println!("❌ [{}] Invalid image format: {}", request_id, ext);
                    return Err(AppError::InvalidImageFormat);
                }

                let ext_clone = ext.clone();
                image_extension = Some(ext_clone);
                image_data = Some(data.to_vec());
                println!("🔍 [{}] Image accepted, extension: {}", request_id, ext);
            }
            "video" => {
                let file_name = field.file_name().unwrap_or("video").to_string();
                let data = field.bytes().await.map_err(|e| {
                    log_error!("[{}] Failed to read video data: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read video data: {}", e))
                })?;

                println!(
                    "🔍 [{}] Video received: {}, size: {} bytes",
                    request_id,
                    file_name,
                    data.len()
                );

                if data.len() as u64 > MAX_VIDEO_SIZE {
                    println!(
                        "❌ [{}] Video too large: {} > {}",
                        request_id,
                        data.len(),
                        MAX_VIDEO_SIZE
                    );
                    // ✅ FIX: Correct error message to match actual limit
                    return Err(AppError::invalid_data("Video too large (max 100MB)"));
                }

                let ext = std::path::Path::new(&file_name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if !ALLOWED_VIDEO_EXTENSIONS.contains(&ext.as_str()) {
                    println!("❌ [{}] Invalid video format: {}", request_id, ext);
                    return Err(AppError::invalid_data("Invalid video format"));
                }

                let ext_clone = ext.clone();
                video_extension = Some(ext_clone);
                video_data = Some(data.to_vec());
                println!("🔍 [{}] Video accepted, extension: {}", request_id, ext);
            }
            "videoThumbnail" => {
                let data = field.bytes().await.map_err(|e| {
                    log_error!("[{}] Failed to read video thumbnail: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read video thumbnail: {}", e))
                })?;
                video_thumbnail_data = Some(data.to_vec());
                println!(
                    "🔍 [{}] Video thumbnail received, size: {} bytes",
                    request_id,
                    data.len()
                );
            }
            _ => {
                println!("🔍 [{}] Unknown field: {}", request_id, field_name);
            }
        }
    }

    if user_id.trim().is_empty() || user_name.trim().is_empty() {
        return Err(AppError::InvalidUserData);
    }

    if caption.is_none() && image_data.is_none() && video_data.is_none() {
        return Err(AppError::invalid_data(
            "Please add a caption, image, or video",
        ));
    }

    let collection: Collection<Post> = state.db.collection("posts");
    let storage_service = &state.storage_service;

    let post = match (image_data, video_data) {
        (None, Some(video_data)) => {
            let ext = video_extension.unwrap_or("mp4".to_string());
            let (video_url, firebase_public_id) = storage_service
                .upload_video(&video_data, &user_id, &ext)
                .await
                .map_err(|e| {
                    AppError::InternalServerError(format!("Video upload failed: {}", e))
                })?;

            let thumbnail_url = if let Some(thumb_data) = video_thumbnail_data {
                storage_service
                    .upload_thumbnail(&thumb_data, &user_id)
                    .await
                    .ok()
            } else {
                None
            };

            let duration = 0;
            let size = video_data.len() as i64;

            // ✅ FIX: preserve caption on video-only uploads
            match caption.clone() {
                Some(caption_text) => Post::new_text_video_post(
                    user_id.clone(),
                    user_name.clone(),
                    caption_text,
                    video_url,
                    firebase_public_id,
                    thumbnail_url,
                    Some(duration),
                    Some(size),
                ),
                None => Post::new_video_post(
                    user_id.clone(),
                    user_name.clone(),
                    video_url,
                    firebase_public_id,
                    thumbnail_url,
                    Some(duration),
                    Some(size),
                ),
            }
        }
        (Some(image_data), Some(video_data)) => {
            let ext = video_extension.unwrap_or("mp4".to_string());
            let (video_url, firebase_public_id) = storage_service
                .upload_video(&video_data, &user_id, &ext)
                .await
                .map_err(|e| {
                    AppError::InternalServerError(format!("Video upload failed: {}", e))
                })?;

            let thumbnail_url = if let Some(thumb_data) = video_thumbnail_data {
                storage_service
                    .upload_thumbnail(&thumb_data, &user_id)
                    .await
                    .ok()
            } else {
                None
            };

            let img_ext = image_extension.unwrap_or("jpg".to_string());
            let (image_url, firebase_image_public_id) = storage_service
                .upload_image(&image_data, &user_id, &img_ext)
                .await
                .map_err(|e| {
                    AppError::InternalServerError(format!("Image upload failed: {}", e))
                })?;

            let duration = 0;
            let size = video_data.len() as i64;

            let mut post = Post::new_text_video_post(
                user_id.clone(),
                user_name.clone(),
                caption.clone().unwrap_or_default(),
                video_url,
                firebase_public_id,
                thumbnail_url,
                Some(duration),
                Some(size),
            );

            post.image_url = Some(image_url);
            post.firebase_image_public_id = Some(firebase_image_public_id);
            post.image_format = Some(img_ext);
            post.post_type = PostType::TextAndVideo;

            post
        }
        (Some(image_data), None) => {
            let ext = image_extension.unwrap_or("jpg".to_string());

            let (image_url, firebase_image_public_id) = storage_service
                .upload_image(&image_data, &user_id, &ext)
                .await
                .map_err(|e| {
                    AppError::InternalServerError(format!("Image upload failed: {}", e))
                })?;

            let image_url_clone1 = image_url.clone();
            let image_url_clone2 = image_url.clone();
            let firebase_image_public_id_clone = firebase_image_public_id.clone();

            match caption {
                Some(caption_text) => {
                    let mut post = Post::new_text_image_post(
                        user_id.clone(),
                        user_name.clone(),
                        caption_text,
                        image_url,
                        "".to_string(),
                        ext.clone(),
                        Some(image_url_clone1),
                        Some(firebase_image_public_id_clone),
                    );
                    post.cloudinary_public_id = None;
                    post
                }
                None => {
                    let mut post = Post::new_image_post(
                        user_id.clone(),
                        user_name.clone(),
                        image_url_clone2,
                        "".to_string(),
                        ext,
                        Some(image_url.clone()),
                        Some(firebase_image_public_id),
                    );
                    post.cloudinary_public_id = None;
                    post
                }
            }
        }
        (None, None) => Post::new_text_post(
            user_id.clone(),
            user_name.clone(),
            caption.unwrap_or_else(|| "".to_string()),
        ),
    };

    let insert_result = collection.insert_one(&post).await?;
    let post_response = PostResponse::from(post.clone());

    let state_clone = state.clone();
    let user_id_clone = user_id.clone();
    let user_name_clone = user_name.clone();
    let caption_text = post
        .caption
        .clone()
        .unwrap_or_else(|| "No caption".to_string());
    let post_id_hex = insert_result
        .inserted_id
        .as_object_id()
        .map(|id| id.to_hex())
        .unwrap_or_default();
    let has_image = post.image_url.is_some();
    let has_video = post.video_url.is_some();

    tokio::spawn(async move {
        if let Some(fcm_service) = &state_clone.fcm_service {
            let all_user_ids = get_all_user_ids(&state_clone, Some(&user_id_clone)).await;

            let post_preview = if caption_text.len() > 50 {
                format!("{}...", &caption_text[0..50])
            } else {
                caption_text.clone()
            };

            let media_type = if has_video && has_image {
                "image_and_video"
            } else if has_video {
                "video"
            } else if has_image {
                "image"
            } else {
                "text"
            };

            if !all_user_ids.is_empty() {
                let _ = fcm_service
                    .send_to_multiple_users(
                        &state_clone,
                        all_user_ids,
                        &format!("📱 New {} post from {}!", media_type, user_name_clone),
                        &post_preview,
                        serde_json::json!({
                            "post_id": post_id_hex,
                            "user_id": user_id_clone,
                            "user_name": user_name_clone,
                            "caption_preview": post_preview,
                            "has_image": has_image,
                            "has_video": has_video,
                            "post_type": media_type,
                            "type": "new_post",
                            "timestamp": Utc::now().to_rfc3339(),
                        }),
                        "new_post",
                    )
                    .await;
            }
        }
    });

    Ok(Json(json!({
        "success": true,
        "message": "Post created successfully",
        "post": post_response
    })))
}

// ========== GET POSTS ==========
pub async fn get_posts(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!("[{}] Starting get_posts handler", request_id);

    let collection: Collection<Post> = state.db.collection("posts");

    let if_modified_since: Option<chrono::DateTime<Utc>> = headers
        .get("If-Modified-Since")
        .and_then(|value| value.to_str().ok())
        .and_then(|str| parse_if_modified_since(str));

    let mut filter = doc! {};

    if let Some(user_id) = &params.user_id {
        filter.insert("user_id", user_id);
    }

    if let Some(post_type) = &params.post_type {
        let post_type_enum = match post_type.as_str() {
            "text" => "Text",
            "image" => "Image",
            "video" => "Video",
            "text_and_image" => "TextAndImage",
            "text_and_video" => "TextAndVideo",
            _ => return Err(AppError::invalid_data("Invalid post_type")),
        };
        filter.insert("post_type", post_type_enum);
    }

    if let Some(since) = if_modified_since {
        filter.insert("last_modified", doc! { "$gt": since });

        let new_post_count = collection.count_documents(filter.clone()).await?;

        if new_post_count == 0 {
            return Ok(Json(json!({
                "success": true,
                "cached": true,
                "message": "No new content",
                "timestamp": since.timestamp()
            })));
        }
    }

    let page = params.page.unwrap_or(1).max(1);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .min(MAX_PAGE_SIZE)
        .max(1);
    let skip = (page - 1) * limit;

    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = (total_count as f64 / limit as f64).ceil() as i64;

    let options = FindOptions::builder()
        .sort(doc! { "last_modified": -1 })
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;

    let post_responses: Vec<PostResponse> = posts.into_iter().map(PostResponse::from).collect();

    let latest_timestamp = if !post_responses.is_empty() {
        post_responses
            .iter()
            .map(|p| p.timestamp)
            .max()
            .unwrap_or(Utc::now().timestamp())
    } else {
        Utc::now().timestamp()
    };

    let text_posts_count = collection
        .count_documents(doc! { "post_type": "Text" })
        .await?;
    let image_posts_count = collection
        .count_documents(doc! { "post_type": "Image" })
        .await?;
    let video_posts_count = collection
        .count_documents(doc! { "post_type": "Video" })
        .await?;
    let text_image_posts_count = collection
        .count_documents(doc! { "post_type": "TextAndImage" })
        .await?;
    let text_video_posts_count = collection
        .count_documents(doc! { "post_type": "TextAndVideo" })
        .await?;

    log_info!(
        "[{}] get_posts completed. Found {} posts",
        request_id,
        post_responses.len()
    );

    Ok(Json(json!({
        "success": true,
        "posts": post_responses,
        "timestamp": latest_timestamp,
        "cache_valid": if_modified_since.is_some(),
        "stats": {
            "text_posts": text_posts_count,
            "image_posts": image_posts_count,
            "video_posts": video_posts_count,
            "text_image_posts": text_image_posts_count,
            "text_video_posts": text_video_posts_count,
            "total": total_count
        },
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

// ========== GET POST BY ID ==========
pub async fn get_post_by_id(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_post_by_id called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    let post_response = PostResponse::from(post);

    Ok(Json(json!({
        "success": true,
        "post": post_response
    })))
}

// ========== DELETE POST (Pure Firebase) ==========
pub async fn delete_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>, // Added for ownership validation
) -> Result<Json<serde_json::Value>> {
    println!("🔍 delete_post called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let filter = doc! { "_id": object_id };

    let post = match collection.find_one(filter.clone()).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    // ✅ FIX: Verify ownership before allowing deletion
    if post.user_id != payload.user_id {
        return Err(AppError::invalid_data(
            "You can only delete your own posts",
        ));
    }

    let storage_service = &state.storage_service;

    // Delete image from Firebase if exists
    if post.has_image() {
        if let Some(ref public_id) = post.firebase_image_public_id {
            println!("🔍 Deleting Firebase image: {}", public_id);
            let _ = storage_service.delete_file(public_id).await;
        }
    }

    // Delete video from Firebase Storage if exists
    if post.has_video() {
        if let Some(ref public_id) = post.firebase_public_id {
            println!("🔍 Deleting Firebase video: {}", public_id);
            let _ = storage_service.delete_file(public_id).await;
        }
    }

    let delete_result = collection.delete_one(filter).await?;

    if delete_result.deleted_count == 0 {
        return Err(AppError::PostNotFound);
    }

    println!("✅ delete_post completed for post: {}", post_id);
    Ok(Json(json!({
        "success": true,
        "message": "Post deleted successfully",
        "post_id": post_id
    })))
}

// ========== LIKE POST ==========
pub async fn like_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 like_post called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if post.liked_by.contains(&payload.user_id) {
        let post_response = PostResponse::from(post);
        return Ok(Json(json!({
            "success": true,
            "message": "Post already liked by user",
            "post": post_response
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
        return Err(AppError::PostNotFound);
    }

    match collection.find_one(doc! { "_id": object_id }).await? {
        Some(updated_post) => {
            let state_clone = state.clone();
            let liker_name = payload.user_name.clone().unwrap_or_else(|| "Someone".to_string());
            let post_id_clone = post_id.clone();
            let likes_count = updated_post.likes_count;

            tokio::spawn(async move {
                if let Some(fcm_service) = &state_clone.fcm_service {
                    let all_user_ids = get_all_user_ids(&state_clone, Some(&payload.user_id)).await;

                    if !all_user_ids.is_empty() {
                        let _ = fcm_service
                            .send_to_multiple_users(
                                &state_clone,
                                all_user_ids,
                                &format!("❤️ {} liked a post", liker_name),
                                &format!("Now {} people like this post", likes_count),
                                serde_json::json!({
                                    "post_id": post_id_clone,
                                    "liker_id": payload.user_id,
                                    "liker_name": liker_name,
                                    "likes_count": likes_count,
                                    "type": "post_like",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "post_like",
                            )
                            .await;
                    }
                }
            });

            let post_response = PostResponse::from(updated_post);
            Ok(Json(json!({
                "success": true,
                "message": "Post liked successfully",
                "post": post_response
            })))
        }
        None => Err(AppError::PostNotFound),
    }
}

// ========== UNLIKE POST ==========
pub async fn unlike_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 unlike_post called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if !post.liked_by.contains(&payload.user_id) {
        let post_response = PostResponse::from(post);
        return Ok(Json(json!({
            "success": true,
            "message": "Post not liked by user",
            "post": post_response
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
        return Err(AppError::PostNotFound);
    }

    match collection.find_one(doc! { "_id": object_id }).await? {
        Some(updated_post) => {
            let post_response = PostResponse::from(updated_post);
            Ok(Json(json!({
                "success": true,
                "message": "Post unliked successfully",
                "post": post_response
            })))
        }
        None => Err(AppError::PostNotFound),
    }
}

// ========== CREATE COMMENT (WITH REPLY SUPPORT) ==========
pub async fn create_comment(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 create_comment called for post: {}", post_id);

    // ✅ FIX: Validate user_id and user_name are non-empty
    if payload.user_id.trim().is_empty() || payload.user_name.trim().is_empty() {
        return Err(AppError::InvalidUserData);
    }

    if payload.comment.trim().is_empty() {
        return Err(AppError::invalid_data("Comment cannot be empty"));
    }

    let comment_collection: Collection<Comment> = state.db.collection("comments");
    let post_collection: Collection<Post> = state.db.collection("posts");

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

    // Create comment with parent_comment_id support
    let comment = Comment::new(
        post_id.clone(),
        payload.user_id.clone(),
        payload.user_name.clone(),
        payload.comment.clone(),
        payload.parent_comment_id.clone(),
    );

    let insert_result = comment_collection.insert_one(&comment).await?;

    if let Some(comment_id) = insert_result.inserted_id.as_object_id() {
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

        // Send notifications
        let state_clone = state.clone();
        let commenter_name = payload.user_name.clone();
        let comment_text = payload.comment.clone();
        let post_id_clone = post_id.clone();
        let comment_id_hex = comment_id.to_hex();
        let parent_comment_id = payload.parent_comment_id.clone();

        tokio::spawn(async move {
            if let Some(fcm_service) = &state_clone.fcm_service {
                // Get parent comment author for reply notifications
                let mut parent_author_id: Option<String> = None;
                let mut parent_author_name: Option<String> = None;

                if let Some(ref parent_id) = parent_comment_id {
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

                let all_user_ids = get_all_user_ids(&state_clone, Some(&payload.user_id)).await;

                let comment_preview = if comment_text.len() > 100 {
                    format!("{}...", &comment_text[0..100])
                } else {
                    comment_text.clone()
                };

                if !all_user_ids.is_empty() {
                    // Determine notification type
                    let (notification_type, title, body) = if parent_comment_id.is_some() {
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
                                "comment_id": comment_id_hex,
                                "commenter_id": payload.user_id,
                                "commenter_name": commenter_name,
                                "comment_preview": &comment_preview,
                                "parent_comment_id": parent_comment_id,
                                "parent_author_id": parent_author_id,
                                "is_reply": parent_comment_id.is_some(),
                                "type": notification_type,
                                "timestamp": Utc::now().to_rfc3339(),
                            }),
                            notification_type,
                        )
                        .await;
                }

                // Send specific notification to parent comment author if reply
                if let Some(parent_author_id) = parent_author_id {
                    if parent_author_id != payload.user_id {
                        let _ = fcm_service
                            .send_to_user(
                                &state_clone,
                                &parent_author_id,
                                &format!("💬 {} replied to your comment", commenter_name),
                                &comment_preview,
                                serde_json::json!({
                                    "post_id": post_id_clone,
                                    "comment_id": comment_id_hex,
                                    "commenter_id": payload.user_id,
                                    "commenter_name": commenter_name,
                                    "comment_preview": &comment_preview,
                                    "parent_comment_id": parent_comment_id,
                                    "type": "comment_reply",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "comment_reply",
                            )
                            .await;
                    }
                }
            }
        });

        let comment_response = CommentResponse::from(comment);

        Ok(Json(json!({
            "success": true,
            "message": "Comment created successfully",
            "comment": comment_response
        })))
    } else {
        Err(AppError::service("Failed to create comment"))
    }
}

// ========== GET COMMENTS ==========
pub async fn get_comments(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_comments called for post: {}", post_id);
    let collection: Collection<Comment> = state.db.collection("comments");

    let page = params.page.unwrap_or(1).max(1);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .min(MAX_PAGE_SIZE)
        .max(1);
    let skip = (page - 1) * limit;

    let total_count = collection
        .count_documents(doc! { "post_id": &post_id })
        .await? as i64;
    let total_pages = (total_count as f64 / limit as f64).ceil() as i64;

    let options = FindOptions::builder()
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection
        .find(doc! { "post_id": &post_id })
        .with_options(options)
        .await?;
    let comments: Vec<Comment> = cursor.try_collect().await?;

    let comment_responses: Vec<CommentResponse> =
        comments.into_iter().map(CommentResponse::from).collect();

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

// ========== UPDATE COMMENT ==========
pub async fn update_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 update_comment called for comment: {}", comment_id);
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
            Ok(Json(json!({
                "success": true,
                "message": "Comment updated successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ========== DELETE COMMENT ==========
pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 delete_comment called for comment: {}", comment_id);
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

    if comment.user_id != payload.user_id {
        return Err(AppError::invalid_data(
            "You can only delete your own comments",
        ));
    }

    let result = comment_collection
        .delete_one(doc! { "_id": object_id })
        .await?;

    if result.deleted_count > 0 {
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

        Ok(Json(json!({
            "success": true,
            "message": "Comment deleted successfully",
            "comment_id": comment_id
        })))
    } else {
        Err(AppError::invalid_data("Comment not found"))
    }
}

// ========== LIKE COMMENT ==========
pub async fn like_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 like_comment called for comment: {}", comment_id);
    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

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
            let comment_response = CommentResponse::from(updated_comment);
            Ok(Json(json!({
                "success": true,
                "message": "Comment liked successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ========== UNLIKE COMMENT ==========
pub async fn unlike_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequestWithUser>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 unlike_comment called for comment: {}", comment_id);
    let collection: Collection<Comment> = state.db.collection("comments");

    let object_id = match ObjectId::parse_str(&comment_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::invalid_data("Invalid comment ID")),
    };

    let comment = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(comment) => comment,
        None => return Err(AppError::invalid_data("Comment not found")),
    };

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
            Ok(Json(json!({
                "success": true,
                "message": "Comment unliked successfully",
                "comment": comment_response
            })))
        }
        None => Err(AppError::invalid_data("Comment not found after update")),
    }
}

// ========== SEARCH POSTS ==========
pub async fn search_posts(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 search_posts called with query: {:?}", params.q);
    let collection: Collection<Post> = state.db.collection("posts");

    let mut filter = doc! {};

    if let Some(query) = &params.q {
        filter.insert(
            "caption",
            doc! {
                "$regex": query,
                "$options": "i"
            },
        );
    }

    if let Some(user_id) = &params.user_id {
        filter.insert("user_id", user_id);
    }

    if let Some(post_type) = &params.post_type {
        let post_type_enum = match post_type.as_str() {
            "text" => "Text",
            "image" => "Image",
            "video" => "Video",
            "text_and_image" => "TextAndImage",
            "text_and_video" => "TextAndVideo",
            _ => return Err(AppError::invalid_data("Invalid post_type")),
        };
        filter.insert("post_type", post_type_enum);
    }

    if let Some(start_date_str) = &params.start_date {
        if let Ok(start_date) = chrono::DateTime::parse_from_rfc3339(start_date_str) {
            filter.insert(
                "created_at",
                doc! { "$gte": start_date.with_timezone(&Utc) },
            );
        }
    }

    if let Some(end_date_str) = &params.end_date {
        if let Ok(end_date) = chrono::DateTime::parse_from_rfc3339(end_date_str) {
            filter.insert("created_at", doc! { "$lte": end_date.with_timezone(&Utc) });
        }
    }

    let page = params.page.unwrap_or(1).max(1);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .min(MAX_PAGE_SIZE)
        .max(1);
    let skip = (page - 1) * limit;

    let total_count = collection.count_documents(filter.clone()).await? as i64;
    let total_pages = (total_count as f64 / limit as f64).ceil() as i64;

    let options = FindOptions::builder()
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .build();

    let cursor = collection.find(filter).with_options(options).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;

    let post_responses: Vec<PostResponse> = posts.into_iter().map(PostResponse::from).collect();

    Ok(Json(json!({
        "success": true,
        "posts": post_responses,
        "search_params": {
            "q": params.q,
            "user_id": params.user_id,
            "post_type": params.post_type,
            "start_date": params.start_date,
            "end_date": params.end_date,
            "page": page,
            "limit": limit
        },
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

// ========== DELETE POSTS BY USER (Pure Firebase) ==========
pub async fn delete_posts_by_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(payload): Json<DeletePostsByUserRequest>, // Added for authorization
) -> Result<Json<serde_json::Value>> {
    println!("🔍 delete_posts_by_user called for user: {}", user_id);

    // ✅ FIX: Verify authorization - only allow users to delete their own posts
    if payload.requesting_user_id != user_id {
        return Err(AppError::invalid_data(
            "You can only delete your own posts",
        ));
    }

    let collection: Collection<Post> = state.db.collection("posts");

    let filter = doc! { "user_id": &user_id };

    let cursor = collection.find(filter.clone()).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;

    if posts.is_empty() {
        return Ok(Json(json!({
            "success": true,
            "message": "No posts found for user",
            "deleted_count": 0
        })));
    }

    let storage_service = &state.storage_service;
    let mut deleted_from_firebase = 0;

    for post in &posts {
        // Delete image from Firebase if exists
        if post.has_image() {
            if let Some(public_id) = &post.firebase_image_public_id {
                println!("🔍 Deleting Firebase image: {}", public_id);
                let _ = storage_service.delete_file(public_id).await;
                deleted_from_firebase += 1;
            }
        }
        // Delete video from Firebase if exists
        if post.has_video() {
            if let Some(public_id) = &post.firebase_public_id {
                println!("🔍 Deleting Firebase video: {}", public_id);
                let _ = storage_service.delete_file(public_id).await;
                deleted_from_firebase += 1;
            }
        }
    }

    let delete_result = collection.delete_many(filter).await?;

    println!(
        "✅ delete_posts_by_user completed. Deleted {} posts from DB, {} from Firebase",
        delete_result.deleted_count, deleted_from_firebase
    );
    Ok(Json(json!({
        "success": true,
        "message": "All user posts deleted successfully",
        "deleted_from_db": delete_result.deleted_count,
        "deleted_from_firebase": deleted_from_firebase,
        "user_id": user_id
    })))
}

// ========== GET POST VIDEO ==========
pub async fn get_post_video(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_post_video called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if !post.has_video() {
        return Err(AppError::invalid_data("This post has no video"));
    }

    Ok(Json(json!({
        "success": true,
        "post_id": post_id,
        "video_url": post.video_url,
        "video_duration": post.video_duration,
        "video_size": post.video_size,
    })))
}

// ========== GET POST VIDEO THUMBNAIL ==========
pub async fn get_post_video_thumbnail(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 get_post_video_thumbnail called for post: {}", post_id);
    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let post = match collection.find_one(doc! { "_id": object_id }).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if !post.has_video() {
        return Err(AppError::invalid_data("This post has no video"));
    }

    Ok(Json(json!({
        "success": true,
        "post_id": post_id,
        "thumbnail_url": post.video_thumbnail_url,
    })))
}