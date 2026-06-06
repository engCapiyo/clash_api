use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use futures_util::TryStreamExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use mongodb::{
    bson::{doc, oid::ObjectId, DateTime},
    Collection,
};
use serde_json::json;

use crate::models::user::{
    AuthResponse, Claims, CreateUserRequest, UpdateUserPointsRequest, User, UserResponse,
};
use crate::state::AppState;

// ============================================================================
// HELPER: Generate JWT Token
// ============================================================================

fn generate_token(user_id: &str, username: &str, phone: &str) -> String {
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        phone: phone.to_string(),
        exp: (Utc::now().timestamp() + 86400) as usize,
    };

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .unwrap_or_else(|_| "".to_string())
}

// ============================================================================
// REGISTER NEW USER
// ============================================================================

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    println!("📝 Registering user with phone: {}", payload.phone);

    let collection: Collection<User> = state.db.collection("users");

    // Check if phone already exists
    match collection.find_one(doc! { "phone": &payload.phone }).await {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "success": false,
                    "message": "Phone number already registered"
                })),
            )
                .into_response();
        }
        Err(e) => {
            println!("❌ Database error checking phone: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Database error"
                })),
            )
                .into_response();
        }
        _ => {}
    }

    // Check if username already exists
    match collection
        .find_one(doc! { "username": &payload.username })
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "success": false,
                    "message": "Username already taken"
                })),
            )
                .into_response();
        }
        Err(e) => {
            println!("❌ Database error checking username: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Database error"
                })),
            )
                .into_response();
        }
        _ => {}
    }

    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    let user = User {
        id: None,
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        balance: 0.0,
        created_at: now,
        updated_at: now,
        is_admin: false,
        season_points: 0,
        correct_votes: 0,
        total_votes: 0,
    };

    match collection.insert_one(&user).await {
        Ok(inserted) => {
            let inserted_id = inserted.inserted_id.as_object_id().unwrap();
            println!(
                "✅ User created: {} with phone: {}",
                payload.username, payload.phone
            );

            let user_response = UserResponse {
                id: inserted_id.to_hex(),
                username: payload.username,
                phone: payload.phone,
                balance: 0.0,
                is_admin: false,
                season_points: 0,
                correct_votes: 0,
                total_votes: 0,
            };

            let token = generate_token(
                &user_response.id,
                &user_response.username,
                &user_response.phone,
            );

            (
                StatusCode::CREATED,
                Json(json!({
                    "success": true,
                    "user": user_response,
                    "token": token
                })),
            )
                .into_response()
        }
        Err(e) => {
            println!("❌ Failed to create user: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to create user: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// LOGIN / GET USER BY PHONE
// ============================================================================

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    println!("🔍 Login attempt with phone: {}", payload.phone);

    let collection: Collection<User> = state.db.collection("users");

    match collection.find_one(doc! { "phone": &payload.phone }).await {
        Ok(Some(user)) => {
            println!("✅ User found: {}", user.username);

            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
                is_admin: user.is_admin,
                season_points: user.season_points,
                correct_votes: user.correct_votes,
                total_votes: user.total_votes,
            };

            let token = generate_token(
                &user_response.id,
                &user_response.username,
                &user_response.phone,
            );

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user": user_response,
                    "token": token
                })),
            )
                .into_response()
        }
        Ok(None) => {
            println!("❌ User not found with phone: {}", payload.phone);
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "success": false,
                    "message": "User not found"
                })),
            )
                .into_response()
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// GET USER BY ID
// ============================================================================

pub async fn get_user_by_id(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    println!("🔍 Looking for user by ID: {}", user_id);

    let collection: Collection<User> = state.db.collection("users");

    let object_id = match ObjectId::parse_str(&user_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": "Invalid user ID format"
                })),
            )
                .into_response();
        }
    };

    match collection.find_one(doc! { "_id": object_id }).await {
        Ok(Some(user)) => {
            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
                is_admin: user.is_admin,
                season_points: user.season_points,
                correct_votes: user.correct_votes,
                total_votes: user.total_votes,
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user": user_response
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "User not found"
            })),
        )
            .into_response(),
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// GET USER BY USERNAME
// ============================================================================

pub async fn get_user_by_username(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    println!("🔍 Looking for user by username: {}", username);

    let collection: Collection<User> = state.db.collection("users");

    match collection.find_one(doc! { "username": username }).await {
        Ok(Some(user)) => {
            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
                is_admin: user.is_admin,
                season_points: user.season_points,
                correct_votes: user.correct_votes,
                total_votes: user.total_votes,
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user": user_response
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "User not found"
            })),
        )
            .into_response(),
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// GET ALL USERS
// ============================================================================

pub async fn get_all_users(State(state): State<AppState>) -> impl IntoResponse {
    println!("📥 Getting all users");

    let collection: Collection<User> = state.db.collection("users");

    match collection.find(doc! {}).await {
        Ok(cursor) => {
            let users: Vec<User> = match cursor.try_collect().await {
                Ok(users) => users,
                Err(e) => {
                    println!("❌ Failed to collect users: {}", e);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "success": false,
                            "message": format!("Failed to fetch users: {}", e)
                        })),
                    )
                        .into_response();
                }
            };

            let user_responses: Vec<UserResponse> = users
                .into_iter()
                .filter_map(|user| {
                    Some(UserResponse {
                        id: user.id?.to_hex(),
                        username: user.username,
                        phone: user.phone,
                        balance: user.balance,
                        is_admin: user.is_admin,
                        season_points: user.season_points,
                        correct_votes: user.correct_votes,
                        total_votes: user.total_votes,
                    })
                })
                .collect();

            println!("✅ Found {} users", user_responses.len());

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "users": user_responses
                })),
            )
                .into_response()
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// UPDATE USER POINTS (Called when fixture finalizes)
// ============================================================================

pub async fn update_user_points(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(payload): Json<UpdateUserPointsRequest>,
) -> impl IntoResponse {
    println!("📊 Updating points for user: {}", user_id);

    let collection: Collection<User> = state.db.collection("users");

    let object_id = match ObjectId::parse_str(&user_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": "Invalid user ID format"
                })),
            )
                .into_response();
        }
    };

    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    match collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "season_points": payload.season_points,
                    "correct_votes": payload.correct_votes,
                    "total_votes": payload.total_votes,
                    "updated_at": now,
                }
            },
        )
        .await
    {
        Ok(result) => {
            if result.matched_count == 0 {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "success": false,
                        "message": "User not found"
                    })),
                )
                    .into_response();
            }

            println!("✅ Updated points for user: {}", user_id);

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "User points updated"
                })),
            )
                .into_response()
        }
        Err(e) => {
            println!("❌ Failed to update user points: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to update user points: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// UPDATE USER PHONE NUMBER
// ============================================================================

pub async fn update_user_phone(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let phone = payload.get("phone").and_then(|p| p.as_str()).unwrap_or("");

    if phone.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "Phone number is required"
            })),
        )
            .into_response();
    }

    let collection: Collection<User> = state.db.collection("users");

    let object_id = match ObjectId::parse_str(&user_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": "Invalid user ID format"
                })),
            )
                .into_response();
        }
    };

    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    match collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "phone": phone,
                    "updated_at": now,
                }
            },
        )
        .await
    {
        Ok(result) => {
            if result.matched_count == 0 {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "success": false,
                        "message": "User not found"
                    })),
                )
                    .into_response();
            }

            println!("✅ Updated phone for user: {}", user_id);

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Phone number updated"
                })),
            )
                .into_response()
        }
        Err(e) => {
            println!("❌ Failed to update phone: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to update phone: {}", e)
                })),
            )
                .into_response()
        }
    }
}
