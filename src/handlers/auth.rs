
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use jsonwebtoken::{encode, EncodingKey, Header};
use mongodb::{
    bson::{doc, oid::ObjectId, DateTime},
    Collection,
};
use serde_json::json;

use crate::state::AppState;
use crate::models::user::{User, CreateUserRequest, UserResponse, Claims, AuthResponse};

// ========== REGISTER NEW USER (Phone + Username) ==========
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    println!("📝 Registering user with phone: {}", payload.phone);
    
    let collection: Collection<User> = state.db.collection("users");

    // Check if phone already exists
    let phone_exists = collection
        .find_one(doc! { "phone": &payload.phone })
        .await;

    if let Ok(Some(_)) = phone_exists {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "success": false,
                "message": "Phone number already registered"
            })),
        ).into_response();
    }

    // Check if username already exists
    let username_exists = collection
        .find_one(doc! { "username": &payload.username })
        .await;

    if let Ok(Some(_)) = username_exists {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "success": false,
                "message": "Username already taken"
            })),
        ).into_response();
    }

    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    let user = User {
        id: None,
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        balance: 0.0,
        created_at: now,
        updated_at: now,
    };

    let result = collection.insert_one(&user).await;

    match result {
        Ok(inserted) => {
            let inserted_id = inserted.inserted_id.as_object_id().unwrap();
            println!("✅ User created: {} with phone: {}", payload.username, payload.phone);

            let user_response = UserResponse {
                id: inserted_id.to_hex(),
                username: payload.username,
                phone: payload.phone,
                balance: 0.0,
            };

            let token = generate_token(&user_response.id, &user_response.username, &user_response.phone);

            (
                StatusCode::CREATED,
                Json(json!({
                    "success": true,
                    "user": user_response,
                    "token": token
                })),
            ).into_response()
        }
        Err(e) => {
            println!("❌ Failed to create user: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to create user"
                })),
            ).into_response()
        }
    }
}

// ========== LOGIN / GET USER BY PHONE ==========
pub async fn get_user_by_phone(
    State(state): State<AppState>,
    phone: String,
) -> impl IntoResponse {
    println!("🔍 Looking for user by phone: {}", phone);
    
    let collection: Collection<User> = state.db.collection("users");
    
    match collection.find_one(doc! { "phone": phone }).await {
        Ok(Some(user)) => {
            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
            };
            
            let token = generate_token(&user_response.id, &user_response.username, &user_response.phone);
            
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user": user_response,
                    "token": token
                })),
            ).into_response()
        }
        Ok(None) => {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "success": false,
                    "message": "User not found"
                })),
            ).into_response()
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Database error"
                })),
            ).into_response()
        }
    }
}

// ========== GET USER BY USERNAME ==========
pub async fn get_user_by_username(
    State(state): State<AppState>,
    username: String,
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
            };
            
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user": user_response
                })),
            ).into_response()
        }
        Ok(None) => {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "success": false,
                    "message": "User not found"
                })),
            ).into_response()
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Database error"
                })),
            ).into_response()
        }
    }
}

// ========== GET ALL USERS ==========
pub async fn get_all_users(
    State(state): State<AppState>,
) -> impl IntoResponse {
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
                    ).into_response();
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
            ).into_response()
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Database error: {}", e)
                })),
            ).into_response()
        }
    }
}

// ========== HELPER: Generate JWT Token ==========
fn generate_token(user_id: &str, username: &str, phone: &str) -> String {
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        phone: phone.to_string(),
        exp: (Utc::now().timestamp() + 86400) as usize, // 24 hours
    };
    
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref()))
        .unwrap_or_else(|_| "".to_string())
}