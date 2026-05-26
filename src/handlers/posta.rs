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

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const ALLOWED_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "gif"];
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

// ========== LOGGING MACROS ==========
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

// Helper function to parse If-Modified-Since header
fn parse_if_modified_since(header_value: &str) -> Option<chrono::DateTime<Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(header_value) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(timestamp) = header_value.parse::<i64>() {
        return chrono::DateTime::from_timestamp(timestamp, 0);
    }

    None
}

// ========== POST HANDLERS ==========
pub async fn create_post(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!("[{}] Starting create_post handler", request_id);

    let mut caption: Option<String> = None;
    let mut user_id = String::new();
    let mut user_name = String::new();
    let mut image_data = None;
    let mut file_extension = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        log_error!("[{}] Failed to process multipart field: {}", request_id, e);
        AppError::Multipart(format!("Failed to process multipart field: {}", e))
    })? {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "caption" => {
                let text = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read caption: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read caption: {}", e))
                })?;

                if !text.trim().is_empty() {
                    caption = Some(text);
                }
            }
            "userId" => {
                user_id = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read user_id: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read user_id: {}", e))
                })?;
            }
            "userName" => {
                user_name = field.text().await.map_err(|e| {
                    log_error!("[{}] Failed to read user_name: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read user_name: {}", e))
                })?;
            }
            "image" => {
                let file_name = field.file_name().unwrap_or("image").to_string();
                let data = field.bytes().await.map_err(|e| {
                    log_error!("[{}] Failed to read image data: {}", request_id, e);
                    AppError::Multipart(format!("Failed to read image data: {}", e))
                })?;

                if data.len() as u64 > MAX_FILE_SIZE {
                    return Err(AppError::ImageTooLarge);
                }

                let ext = std::path::Path::new(&file_name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                    return Err(AppError::InvalidImageFormat);
                }

                file_extension = Some(ext.clone());
                image_data = Some(data.to_vec());
            }
            _ => continue,
        }
    }

    if user_id.trim().is_empty() || user_name.trim().is_empty() {
        return Err(AppError::InvalidUserData);
    }

    if caption.is_none() && image_data.is_none() {
        return Err(AppError::invalid_data(
            "Please add a caption or select an image",
        ));
    }

    let collection: Collection<Post> = state.db.collection("posts");

    let post = if let Some(image_data) = image_data {
        let file_extension = file_extension.unwrap();
        let cloudinary_service = &state.cloudinary;
        let public_id = format!("post_{}_{}", user_id, Uuid::new_v4());
        let upload_path = format!("fanclash/posts/{}", user_id);

        let (image_url, cloudinary_public_id) = match cloudinary_service
            .upload_image_with_preset(&image_data, &upload_path, Some(&public_id))
            .await
        {
            Ok(result) => result,
            Err(_) => cloudinary_service
                .upload_image_signed(&image_data, &upload_path, Some(&public_id))
                .await
                .map_err(|e| {
                    AppError::invalid_data(format!("Both upload methods failed. Last error: {}", e))
                })?,
        };

        match caption {
            Some(caption_text) => {
                log_info!("[{}] Creating post with both image and caption", request_id);
                Post::new_text_image_post(
                    user_id.clone(),
                    user_name.clone(),
                    caption_text,
                    image_url,
                    cloudinary_public_id,
                    file_extension,
                )
            }
            None => {
                log_info!("[{}] Creating image-only post", request_id);
                Post::new_image_post(
                    user_id.clone(),
                    user_name.clone(),
                    image_url,
                    cloudinary_public_id,
                    file_extension,
                )
            }
        }
    } else {
        log_info!("[{}] Creating text-only post", request_id);
        Post::new_text_post(user_id.clone(), user_name.clone(), caption.unwrap())
    };

    let insert_result = collection.insert_one(&post).await?;
    let post_response = PostResponse::from(post.clone());

    // ========== SEND FCM NOTIFICATIONS TO ALL USERS ==========
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

    tokio::spawn(async move {
        if let Ok(fcm_service) = crate::services::fcm_service::init_fcm_service().await {
            let all_user_ids = get_all_user_ids(&state_clone, Some(&user_id_clone)).await;

            let post_preview = if caption_text.len() > 50 {
                format!("{}...", &caption_text[0..50])
            } else {
                caption_text.clone()
            };

            let has_image = post.image_url.is_some();

            if !all_user_ids.is_empty() {
                println!(
                    "📱 Notifying ALL {} users about new post from {}",
                    all_user_ids.len(),
                    user_name_clone
                );

                let _ = fcm_service
                    .send_to_multiple_users(
                        &state_clone,
                        all_user_ids,
                        &format!("📱 New post from {}!", user_name_clone),
                        &post_preview,
                        serde_json::json!({
                            "post_id": post_id_hex,
                            "user_id": user_id_clone,
                            "user_name": user_name_clone,
                            "caption_preview": post_preview,
                            "has_image": has_image,
                            "type": "new_post",
                            "timestamp": Utc::now().to_rfc3339(),
                        }),
                        "new_post",
                    )
                    .await;
            }
        }
    });
    // ========== END FCM NOTIFICATION ==========

    Ok(Json(json!({
        "success": true,
        "message": "Post created successfully",
        "post": post_response
    })))
}

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
            "text_and_image" => "TextAndImage",
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
    let text_image_posts_count = collection
        .count_documents(doc! { "post_type": "TextAndImage" })
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
            "text_image_posts": text_image_posts_count,
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

pub async fn search_posts(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!("[{}] Starting search_posts handler", request_id);

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
            "text_and_image" => "TextAndImage",
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

pub async fn migrate_posts(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!("[{}] Starting post migration", request_id);

    let collection: Collection<Post> = state.db.collection("posts");

    let filter = doc! { "post_type": { "$exists": false } };
    let cursor = collection.find(filter.clone()).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;
    let total_posts = posts.len();

    let mut updated_count = 0;
    let mut skipped_count = 0;

    for post in posts {
        let post_type_str = match (post.caption.is_some(), post.image_url.is_some()) {
            (true, true) => "TextAndImage",
            (true, false) => "Text",
            (false, true) => "Image",
            (false, false) => "Text",
        };

        let _post_type = match post_type_str {
            "Text" => PostType::Text,
            "Image" => PostType::Image,
            "TextAndImage" => PostType::TextAndImage,
            _ => PostType::Text,
        };

        let update = doc! {
            "$set": { "post_type": post_type_str }
        };

        if let Some(id) = post._id {
            let result = collection.update_one(doc! { "_id": id }, update).await?;

            if result.modified_count > 0 {
                updated_count += 1;
                log_info!(
                    "[{}] Updated post {} with type {}",
                    request_id,
                    id,
                    post_type_str
                );
            } else {
                skipped_count += 1;
            }
        }
    }

    log_info!(
        "[{}] Migration complete. Updated: {}, Skipped: {}, Total: {}",
        request_id,
        updated_count,
        skipped_count,
        total_posts
    );

    Ok(Json(json!({
        "success": true,
        "message": "Migration completed",
        "stats": {
            "total_posts_found": total_posts,
            "updated": updated_count,
            "skipped": skipped_count
        }
    })))
}

pub async fn get_post_by_id(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_post_by_id handler. Post ID: {}",
        request_id,
        post_id
    );

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

pub async fn get_posts_by_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_posts_by_user handler. User ID: {}",
        request_id,
        user_id
    );

    let collection: Collection<Post> = state.db.collection("posts");

    let mut filter = doc! { "user_id": &user_id };

    if let Some(post_type) = &params.post_type {
        let post_type_enum = match post_type.as_str() {
            "text" => "Text",
            "image" => "Image",
            "text_and_image" => "TextAndImage",
            _ => return Err(AppError::invalid_data("Invalid post_type")),
        };
        filter.insert("post_type", post_type_enum);
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

    let text_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "Text"
        })
        .await?;

    let image_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "Image"
        })
        .await?;

    let text_image_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "TextAndImage"
        })
        .await?;

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "posts": post_responses,
        "user_stats": {
            "text_posts": text_posts,
            "image_posts": image_posts,
            "text_image_posts": text_image_posts,
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

pub async fn update_post_caption(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    axum::extract::Json(payload): axum::extract::Json<UpdateCaptionRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting update_post_caption handler. Post ID: {}",
        request_id,
        post_id
    );

    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let filter = doc! { "_id": object_id };
    let update = doc! {
        "$set": {
            "caption": Some(payload.caption.clone()),
            "updated_at": Utc::now(),
            "last_modified": Utc::now()
        }
    };

    let result = collection.update_one(filter, update).await?;

    if result.matched_count == 0 {
        return Err(AppError::PostNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Post caption updated successfully",
        "caption": payload.caption
    })))
}

pub async fn delete_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting delete_post handler. Post ID: {}",
        request_id,
        post_id
    );

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

    if post.has_image() {
        let cloudinary_service = &state.cloudinary;
        if let Some(public_id) = post.cloudinary_public_id {
            let _ = cloudinary_service.delete_image(&public_id).await;
        }
    }

    let delete_result = collection.delete_one(filter).await?;

    if delete_result.deleted_count == 0 {
        return Err(AppError::PostNotFound);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Post deleted successfully",
        "post_id": post_id
    })))
}

pub async fn delete_posts_by_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting delete_posts_by_user handler. User ID: {}",
        request_id,
        user_id
    );

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

    let cloudinary_service = &state.cloudinary;
    let mut deleted_from_cloudinary = 0;

    for post in &posts {
        if post.has_image() {
            if let Some(public_id) = &post.cloudinary_public_id {
                let _ = cloudinary_service.delete_image(public_id).await;
                deleted_from_cloudinary += 1;
            }
        }
    }

    let delete_result = collection.delete_many(filter).await?;

    Ok(Json(json!({
        "success": true,
        "message": "All user posts deleted successfully",
        "deleted_from_db": delete_result.deleted_count,
        "deleted_from_cloudinary": deleted_from_cloudinary,
        "user_id": user_id
    })))
}

pub async fn get_post_thumbnail(
    State(state): State<AppState>,
    Path((post_id, width, height)): Path<(String, u32, u32)>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_post_thumbnail handler. Post ID: {}, Dimensions: {}x{}",
        request_id,
        post_id,
        width,
        height
    );

    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let filter = doc! { "_id": object_id };

    let post = match collection.find_one(filter).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if !post.has_image() {
        return Err(AppError::invalid_data("This post has no image"));
    }

    let cloudinary_service = &state.cloudinary;
    let thumbnail_url = cloudinary_service.generate_thumbnail_url(
        &post.cloudinary_public_id.unwrap(),
        width,
        height,
    );

    Ok(Json(json!({
        "success": true,
        "thumbnail_url": thumbnail_url,
        "post_id": post_id,
        "original_url": post.image_url,
        "dimensions": {
            "width": width,
            "height": height
        }
    })))
}

pub async fn get_post_with_transform(
    State(state): State<AppState>,
    Path((post_id, transformations)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_post_with_transform handler. Post ID: {}, Transformations: {}",
        request_id,
        post_id,
        transformations
    );

    let collection: Collection<Post> = state.db.collection("posts");

    let object_id = match ObjectId::parse_str(&post_id) {
        Ok(oid) => oid,
        Err(_) => return Err(AppError::PostNotFound),
    };

    let filter = doc! { "_id": object_id };

    let post = match collection.find_one(filter).await? {
        Some(post) => post,
        None => return Err(AppError::PostNotFound),
    };

    if !post.has_image() {
        return Err(AppError::invalid_data("This post has no image"));
    }

    let cloudinary_service = &state.cloudinary;
    let transformed_url = cloudinary_service
        .generate_transformed_url(&post.cloudinary_public_id.unwrap(), &transformations);

    Ok(Json(json!({
        "success": true,
        "transformed_url": transformed_url,
        "post_id": post_id,
        "transformations": transformations,
        "original_url": post.image_url
    })))
}

pub async fn get_post_stats(State(state): State<AppState>) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!("[{}] Starting get_post_stats handler", request_id);

    let collection: Collection<Post> = state.db.collection("posts");

    let total_posts: u64 = collection.count_documents(doc! {}).await?;

    let text_posts = collection
        .count_documents(doc! { "post_type": "Text" })
        .await?;
    let image_posts = collection
        .count_documents(doc! { "post_type": "Image" })
        .await?;
    let text_image_posts = collection
        .count_documents(doc! { "post_type": "TextAndImage" })
        .await?;

    let seven_days_ago = Utc::now() - chrono::Duration::days(7);
    let posts_last_week: u64 = collection
        .count_documents(doc! { "created_at": { "$gte": seven_days_ago } })
        .await?;

    let pipeline = vec![
        doc! {
            "$group": {
                "_id": "$user_id",
                "count": { "$sum": 1 },
                "user_name": { "$first": "$user_name" }
            }
        },
        doc! { "$sort": { "count": -1 } },
        doc! { "$limit": 10 },
    ];

    let cursor = collection.aggregate(pipeline).await?;
    let top_users_docs: Vec<Document> = cursor.try_collect().await?;
    let top_users: Vec<JsonValue> = top_users_docs.into_iter().map(document_to_json).collect();

    let hour_pipeline = vec![
        doc! {
            "$group": {
                "_id": { "$hour": "$created_at" },
                "count": { "$sum": 1 }
            }
        },
        doc! { "$sort": { "_id": 1 } },
    ];

    let cursor = collection.aggregate(hour_pipeline).await?;
    let posts_by_hour_docs: Vec<Document> = cursor.try_collect().await?;
    let posts_by_hour: Vec<JsonValue> = posts_by_hour_docs
        .into_iter()
        .map(document_to_json)
        .collect();

    Ok(Json(json!({
        "success": true,
        "stats": {
            "total_posts": total_posts,
            "text_posts": text_posts,
            "image_posts": image_posts,
            "text_image_posts": text_image_posts,
            "posts_last_week": posts_last_week,
            "top_users": top_users,
            "posts_by_hour": posts_by_hour,
            "timestamp": Utc::now().timestamp()
        }
    })))
}

pub async fn get_user_post_stats(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_user_post_stats handler. User ID: {}",
        request_id,
        user_id
    );

    let collection: Collection<Post> = state.db.collection("posts");

    let filter = doc! { "user_id": &user_id };

    let total_posts: u64 = collection.count_documents(filter.clone()).await?;

    let text_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "Text"
        })
        .await?;

    let image_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "Image"
        })
        .await?;

    let text_image_posts = collection
        .count_documents(doc! {
            "user_id": &user_id,
            "post_type": "TextAndImage"
        })
        .await?;

    let latest_post = collection
        .find_one(filter.clone())
        .sort(doc! { "created_at": -1 })
        .await?;
    let first_post = collection
        .find_one(filter.clone())
        .sort(doc! { "created_at": 1 })
        .await?;

    let pipeline = vec![
        doc! { "$match": filter.clone() },
        doc! {
            "$group": {
                "_id": {
                    "year": { "$year": "$created_at" },
                    "month": { "$month": "$created_at" }
                },
                "count": { "$sum": 1 }
            }
        },
        doc! { "$sort": { "_id.year": 1, "_id.month": 1 } },
    ];

    let cursor = collection.aggregate(pipeline).await?;
    let posts_by_month_docs: Vec<Document> = cursor.try_collect().await?;
    let posts_by_month: Vec<JsonValue> = posts_by_month_docs
        .into_iter()
        .map(document_to_json)
        .collect();

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "stats": {
            "total_posts": total_posts,
            "text_posts": text_posts,
            "image_posts": image_posts,
            "text_image_posts": text_image_posts,
            "latest_post": latest_post.and_then(|p| p._id.map(|id| id.to_hex())),
            "first_post": first_post.and_then(|p| p._id.map(|id| id.to_hex())),
            "posts_by_month": posts_by_month,
            "timestamp": Utc::now().timestamp()
        }
    })))
}

// ========== LIKE HANDLERS ==========
pub async fn like_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting like_post handler. Post ID: {}, User ID: {}",
        request_id,
        post_id,
        payload.user_id
    );

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
            // ========== SEND FCM NOTIFICATION TO ALL USERS ==========
            let state_clone = state.clone();
            let post_owner_id = updated_post.user_id.clone();
            let liker_name = payload.user_name.clone();
            let post_id_clone = post_id.clone();
            let likes_count = updated_post.likes_count;

            tokio::spawn(async move {
                if let Ok(fcm_service) = crate::services::fcm_service::init_fcm_service().await {
                    let all_user_ids = get_all_user_ids(&state_clone, Some(&payload.user_id)).await;

                    if !all_user_ids.is_empty() {
                        println!(
                            "📱 Notifying ALL {} users about like on post",
                            all_user_ids.len()
                        );

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
            // ========== END FCM NOTIFICATION ==========

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

pub async fn unlike_post(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting unlike_post handler. Post ID: {}, User ID: {}",
        request_id,
        post_id,
        payload.user_id
    );

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

// ========== COMMENT HANDLERS ==========
pub async fn get_comments(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting get_comments handler. Post ID: {}",
        request_id,
        post_id
    );

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

pub async fn create_comment(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting create_comment handler. Post ID: {}, User ID: {}",
        request_id,
        post_id,
        payload.user_id
    );

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

    let existing_comment = comment_collection
        .find_one(doc! {
            "post_id": &post_id,
            "user_id": &payload.user_id
        })
        .await?;

    if existing_comment.is_some() {
        return Err(AppError::invalid_data(
            "You have already commented on this post. You can edit your existing comment.",
        ));
    }

    let comment = Comment::new(
        post_id.clone(),
        payload.user_id.clone(),
        payload.user_name.clone(),
        payload.comment.clone(),
    );

    let insert_result = comment_collection.insert_one(&comment).await?;

    if let Some(comment_id) = insert_result.inserted_id.as_object_id() {
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

        // ========== SEND FCM NOTIFICATION TO ALL USERS ==========
        let state_clone = state.clone();
        let commenter_name = payload.user_name.clone();
        let comment_text = payload.comment.clone();
        let post_id_clone = post_id.clone();
        let comment_id_hex = comment_id.to_hex();

        tokio::spawn(async move {
            if let Ok(fcm_service) = crate::services::fcm_service::init_fcm_service().await {
                let all_user_ids = get_all_user_ids(&state_clone, Some(&payload.user_id)).await;

                let comment_preview = if comment_text.len() > 100 {
                    format!("{}...", &comment_text[0..100])
                } else {
                    comment_text.clone()
                };

                if !all_user_ids.is_empty() {
                    println!(
                        "📱 Notifying ALL {} users about new comment",
                        all_user_ids.len()
                    );

                    let _ = fcm_service
                        .send_to_multiple_users(
                            &state_clone,
                            all_user_ids,
                            &format!("💬 {} commented on a post", commenter_name),
                            &comment_preview,
                            serde_json::json!({
                                "post_id": post_id_clone,
                                "comment_id": comment_id_hex,
                                "commenter_id": payload.user_id,
                                "commenter_name": commenter_name,
                                "comment_preview": comment_preview,
                                "type": "post_comment",
                                "timestamp": Utc::now().to_rfc3339(),
                            }),
                            "post_comment",
                        )
                        .await;
                }
            }
        });
        // ========== END FCM NOTIFICATION ==========

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

pub async fn update_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting update_comment handler. Comment ID: {}, User ID: {}",
        request_id,
        comment_id,
        payload.user_id
    );

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

pub async fn delete_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting delete_comment handler. Comment ID: {}, User ID: {}",
        request_id,
        comment_id,
        payload.user_id
    );

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

pub async fn like_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting like_comment handler. Comment ID: {}, User ID: {}",
        request_id,
        comment_id,
        payload.user_id
    );

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
            // ========== SEND FCM NOTIFICATION TO ALL USERS ==========
            let state_clone = state.clone();
            let liker_name = payload.user_name.clone();
            let comment_author_id = updated_comment.user_id.clone();
            let post_id_clone = updated_comment.post_id.clone();

            tokio::spawn(async move {
                if let Ok(fcm_service) = crate::services::fcm_service::init_fcm_service().await {
                    let all_user_ids = get_all_user_ids(&state_clone, Some(&payload.user_id)).await;

                    if !all_user_ids.is_empty() {
                        println!(
                            "📱 Notifying ALL {} users about comment like",
                            all_user_ids.len()
                        );

                        let _ = fcm_service
                            .send_to_multiple_users(
                                &state_clone,
                                all_user_ids,
                                &format!("❤️ {} liked a comment", liker_name),
                                "On a post",
                                serde_json::json!({
                                    "post_id": post_id_clone,
                                    "comment_id": comment_id,
                                    "liker_id": payload.user_id,
                                    "liker_name": liker_name,
                                    "comment_author_id": comment_author_id,
                                    "type": "comment_like",
                                    "timestamp": Utc::now().to_rfc3339(),
                                }),
                                "comment_like",
                            )
                            .await;
                    }
                }
            });
            // ========== END FCM NOTIFICATION ==========

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

pub async fn unlike_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<String>,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<serde_json::Value>> {
    let request_id = uuid::Uuid::new_v4();
    log_info!(
        "[{}] Starting unlike_comment handler. Comment ID: {}, User ID: {}",
        request_id,
        comment_id,
        payload.user_id
    );

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
