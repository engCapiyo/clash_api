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
use rand::Rng;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::models::user::{
    AuthResponse, Claims, CreateUserRequest, PinLoginRequest, SetPinRequest,
    UpdateUserPointsRequest, User, UserResponse,
};
use crate::state::AppState;

// ============================================================================
// HELPER: Normalize phone number (country-code agnostic)
// ============================================================================
// Strips everything but digits, drops a leading trunk '0', then keeps only
// the last 9 digits. This makes "+254705306867", "254705306867", and
// "0705306867" all collapse to the same value: "705306867".
fn normalize_phone(phone: &str) -> String {
    let mut cleaned: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if cleaned.starts_with('0') {
        cleaned = cleaned[1..].to_string();
    }
    if cleaned.len() > 9 {
        cleaned = cleaned[cleaned.len() - 9..].to_string();
    }
    cleaned
}

// Builds a Mongo query that matches any stored phone ending in the same
// digits, regardless of how it was originally formatted/stored.
// Safe to format directly into regex: normalize_phone only ever returns
// ASCII digits, so there are no characters that need escaping.
fn phone_query(phone: &str) -> mongodb::bson::Document {
    let normalized = normalize_phone(phone);
    doc! { "phone": { "$regex": format!("{}$", normalized) } }
}

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
// HELPER: Hash PIN with salt
// ============================================================================

fn hash_pin(pin: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{}{}", pin, salt).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_salt() -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
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

    // Check if phone already exists — normalized match, not exact string
    match collection.find_one(phone_query(&payload.phone)).await {
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
        pin_hash: None,
        pin_salt: None,
        is_pin_enabled: false,
        firebase_uid: None,
        auth_methods: vec![],
        last_login: None,
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
                has_pin: false,
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
// LOGIN / GET USER BY PHONE (Firebase flow)
// ============================================================================

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    println!("🔍 Login attempt with phone: {}", payload.phone);

    let collection: Collection<User> = state.db.collection("users");

    match collection.find_one(phone_query(&payload.phone)).await {
        Ok(Some(user)) => {
            println!("✅ User found: {}", user.username);

            // Update last login
            let now = DateTime::from_millis(Utc::now().timestamp_millis());
            let _ = collection
                .update_one(
                    doc! { "_id": user.id.clone().unwrap() },
                    doc! { "$set": { "last_login": now } },
                )
                .await;

            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
                is_admin: user.is_admin,
                season_points: user.season_points,
                correct_votes: user.correct_votes,
                total_votes: user.total_votes,
                has_pin: user.is_pin_enabled,
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
// CHECK IF USER EXISTS (for PIN fallback)
// ============================================================================

pub async fn check_user_exists(
    State(state): State<AppState>,
    Path(phone): Path<String>,
) -> impl IntoResponse {
    println!("🔍 Checking if user exists: {}", phone);

    let collection: Collection<User> = state.db.collection("users");

    match collection.find_one(phone_query(&phone)).await {
        Ok(Some(user)) => {
            println!("✅ User exists: {}", user.username);
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "exists": true,
                    "has_pin": user.is_pin_enabled,
                    "user": {
                        "id": user.id.unwrap().to_hex(),
                        "username": user.username,
                        "phone": user.phone,
                        "has_pin": user.is_pin_enabled,
                    }
                })),
            )
                .into_response()
        }
        Ok(None) => {
            println!("❌ User not found: {}", phone);
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "exists": false,
                    "has_pin": false,
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
// SET PIN FOR USER (after Firebase registration or fallback)
// ============================================================================

pub async fn set_pin(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(payload): Json<SetPinRequest>,
) -> impl IntoResponse {
    println!("🔐 Setting PIN for user: {}", user_id);

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

    // Validate PIN format
    if payload.pin.len() != 4 || !payload.pin.chars().all(|c| c.is_ascii_digit()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "PIN must be exactly 4 digits"
            })),
        )
            .into_response();
    }

    let salt = generate_salt();
    let pin_hash = hash_pin(&payload.pin, &salt);
    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    match collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "pin_hash": pin_hash,
                    "pin_salt": salt,
                    "is_pin_enabled": true,
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

            println!("✅ PIN set for user: {}", user_id);

            // Get updated user
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
                        has_pin: true,
                    };

                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": "PIN set successfully",
                            "user": user_response,
                        })),
                    )
                        .into_response()
                }
                _ => (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": "PIN set successfully",
                    })),
                )
                    .into_response(),
            }
        }
        Err(e) => {
            println!("❌ Failed to set PIN: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to set PIN: {}", e)
                })),
            )
                .into_response()
        }
    }
}

// ============================================================================
// PIN LOGIN (fallback for Firebase failures)
// ============================================================================

pub async fn pin_login(
    State(state): State<AppState>,
    Json(payload): Json<PinLoginRequest>,
) -> impl IntoResponse {
    println!("🔐 PIN login attempt for: {}", payload.phone);

    let collection: Collection<User> = state.db.collection("users");

    // Find user by phone — normalized match
    match collection.find_one(phone_query(&payload.phone)).await {
        Ok(Some(user)) => {
            // Check if PIN is enabled
            if !user.is_pin_enabled {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "PIN not set for this account"
                    })),
                )
                    .into_response();
            }

            // Verify PIN
            let salt = user.pin_salt.as_deref().unwrap_or("");
            let expected_hash = user.pin_hash.as_deref().unwrap_or("");
            let provided_hash = hash_pin(&payload.pin, salt);

            if provided_hash != expected_hash {
                println!("❌ Invalid PIN for user: {}", payload.phone);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "success": false,
                        "message": "Invalid PIN"
                    })),
                )
                    .into_response();
            }

            println!("✅ PIN login successful for: {}", payload.phone);

            // Update last login
            let now = DateTime::from_millis(Utc::now().timestamp_millis());
            let _ = collection
                .update_one(
                    doc! { "_id": user.id.clone().unwrap() },
                    doc! { "$set": { "last_login": now } },
                )
                .await;

            let user_response = UserResponse {
                id: user.id.unwrap().to_hex(),
                username: user.username,
                phone: user.phone,
                balance: user.balance,
                is_admin: user.is_admin,
                season_points: user.season_points,
                correct_votes: user.correct_votes,
                total_votes: user.total_votes,
                has_pin: true,
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
                has_pin: user.is_pin_enabled,
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
                has_pin: user.is_pin_enabled,
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
                        has_pin: user.is_pin_enabled,
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
    let raw_phone = payload.get("phone").and_then(|p| p.as_str()).unwrap_or("");

    if raw_phone.trim().is_empty() {
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

    // Validate: digits-only count (after stripping symbols) must look like
    // a real phone number, not garbage.
    let digit_count = raw_phone.chars().filter(|c| c.is_ascii_digit()).count();
    if digit_count < 9 || digit_count > 15 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "Phone number must contain between 9 and 15 digits"
            })),
        )
            .into_response();
    }

    // Reject anything that isn't digits, spaces, dashes, parens, or a
    // leading '+' — guards against junk data ending up in the phone field.
    let is_valid_format = raw_phone
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '(' | ')'));
    if !is_valid_format {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "Phone number contains invalid characters"
            })),
        )
            .into_response();
    }

    // Store a cleaned version: keep a leading '+' if present, strip
    // everything else that isn't a digit (spaces, dashes, parens).
    let cleaned_phone: String = {
        let has_plus = raw_phone.trim_start().starts_with('+');
        let digits_only: String = raw_phone.chars().filter(|c| c.is_ascii_digit()).collect();
        if has_plus {
            format!("+{}", digits_only)
        } else {
            digits_only
        }
    };

    // Check no *other* user already has this number (normalized match,
    // so "+254705306867" vs "0705306867" are correctly seen as the same).
    match collection.find_one(phone_query(&cleaned_phone)).await {
        Ok(Some(existing)) => {
            if existing.id != Some(object_id) {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "success": false,
                        "message": "Phone number is already in use by another account"
                    })),
                )
                    .into_response();
            }
        }
        Err(e) => {
            println!("❌ Database error checking phone uniqueness: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Database error"
                })),
            )
                .into_response();
        }
        Ok(None) => {}
    }

    let now = DateTime::from_millis(Utc::now().timestamp_millis());

    match collection
        .update_one(
            doc! { "_id": object_id },
            doc! {
                "$set": {
                    "phone": &cleaned_phone,
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

            println!(
                "✅ Updated phone for user: {} -> {}",
                user_id, cleaned_phone
            );

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Phone number updated",
                    "phone": cleaned_phone,
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
// ============================================================================
// UPDATE USER BALANCE (Called by M-Pesa callback)
// ============================================================================

pub async fn update_user_balance_by_phone(
    State(state): State<AppState>,
    phone: &str,
    amount: f64,
) -> Result<(), String> {
    println!("💰 Updating user balance for phone: {}", phone);

    let collection: Collection<User> = state.db.collection("users");

    // Find user by phone (using normalized match)
    let phone_filter = phone_query(phone);

    // Update balance - increment by amount
    let update = doc! {
        "$inc": { "balance": amount },
        "$set": { "updated_at": DateTime::from_millis(Utc::now().timestamp_millis()) }
    };

    match collection.update_one(phone_filter, update).await {
        Ok(result) => {
            if result.matched_count == 0 {
                return Err(format!("User not found with phone: {}", phone));
            }
            println!("✅ Balance updated for phone: {}", phone);
            Ok(())
        }
        Err(e) => {
            println!("❌ Failed to update balance: {}", e);
            Err(format!("Database error: {}", e))
        }
    }
}
