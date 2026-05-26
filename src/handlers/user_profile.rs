use axum::{
    extract::{State, Query, Path},
    response::Json,
};
use serde::Deserialize;
use mongodb::bson::{doc, oid::ObjectId, DateTime as BsonDateTime};
use chrono::Utc;
use mongodb::Collection;
use futures_util::TryStreamExt;
use validator::Validate;

use crate::state::AppState;
use crate::models::user_profile::{UserProfile, CreateUserProfile, UpdateBalanceRequest, UserQuery};
use crate::errors::{AppError, Result};

#[derive(Debug, Deserialize)]
pub struct SaveProfileRequest {
    pub user_id: String,
    pub username: String,
    pub phone: String,
    pub nickname: String,
    pub club_fan: String,
    pub country_fan: String,
    pub balance: f64,
    pub number_of_bets: i32,
}

// Get all user profiles
pub async fn get_user_profiles(
    State(state): State<AppState>,
    Query(query): Query<UserQuery>,
) -> Result<Json<Vec<UserProfile>>> {
    println!("🔍 GET /api/users/profiles called");

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");
    let mut filter = doc! {};

    if let Some(user_id) = &query.user_id {
        filter.insert("user_id", user_id);
    }
    if let Some(phone) = &query.phone {
        filter.insert("phone", phone);
    }
    if let Some(username) = &query.username {
        filter.insert("username", username);
    }

    let cursor = collection.find(filter).await?;
    let mut users: Vec<UserProfile> = cursor.try_collect().await?;

    // Sort by created_at descending (most recent first)
    users.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("✅ Successfully fetched {} user profiles", users.len());
    Ok(Json(users))
}

// Get user profile by ID
pub async fn get_user_profile_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UserProfile>> {
    println!("🔍 GET /api/users/profiles/{}", id);

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    // Try to find by user_id first
    let filter = doc! { "user_id": &id };

    match collection.find_one(filter).await? {
        Some(user) => {
            println!("✅ Found user: {}", user.username);
            Ok(Json(user))
        }
        None => {
            // Try by MongoDB ObjectId
            if let Ok(object_id) = ObjectId::parse_str(&id) {
                let filter = doc! { "_id": object_id };
                match collection.find_one(filter).await? {
                    Some(user) => {
                        println!("✅ Found user by ObjectId: {}", user.username);
                        return Ok(Json(user));
                    }
                    None => {
                        println!("❌ User not found: {}", id);
                        return Err(AppError::DocumentNotFound);
                    }
                }
            }

            println!("❌ User not found: {}", id);
            Err(AppError::DocumentNotFound)
        }
    }
}

// Get user profile by phone
pub async fn get_user_profile_by_phone(
    State(state): State<AppState>,
    Path(phone): Path<String>,
) -> Result<Json<UserProfile>> {
    println!("🔍 GET /api/users/phone/{}", phone);

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    let filter = doc! { "phone": &phone };

    match collection.find_one(filter).await? {
        Some(user) => {
            println!("✅ Found user by phone: {}", user.username);
            Ok(Json(user))
        }
        None => {
            println!("❌ User not found with phone: {}", phone);
            Err(AppError::DocumentNotFound)
        }
    }
}

// Create or update user profile (UPSERT)
pub async fn save_user_profile(
    State(state): State<AppState>,
    Json(payload): Json<SaveProfileRequest>,
) -> Result<Json<UserProfile>> {
    println!("🎯 Saving user profile for: {}", payload.username);

    // Validate required fields
    if payload.user_id.is_empty() {
        return Err(AppError::invalid_data("User ID is required"));
    }

    if payload.phone.is_empty() {
        return Err(AppError::invalid_data("Phone number is required"));
    }

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    // Check if user already exists
    let filter = doc! { "user_id": &payload.user_id };

    let existing_user = collection.find_one(filter.clone()).await?;

    let now = Utc::now();
    let bson_now = BsonDateTime::from_chrono(now);

    let user_profile = UserProfile {
        id: existing_user.as_ref().and_then(|u| u.id.clone()).or(Some(ObjectId::new())),
        user_id: payload.user_id.clone(),
        username: payload.username,
        phone: payload.phone,
        nickname: payload.nickname,
        club_fan: payload.club_fan,
        country_fan: payload.country_fan,
        balance: payload.balance,
        number_of_bets: payload.number_of_bets,
        created_at: existing_user.as_ref()
            .map(|u| u.created_at)
            .unwrap_or(bson_now),
        updated_at: bson_now,
    };

    // Upsert: update if exists, insert if new
    let update = doc! {
        "$set": {
            "username": &user_profile.username,
            "phone": &user_profile.phone,
            "nickname": &user_profile.nickname,
            "club_fan": &user_profile.club_fan,
            "country_fan": &user_profile.country_fan,
            "balance": user_profile.balance,
            "number_of_bets": user_profile.number_of_bets,
            "updated_at": user_profile.updated_at
        },
        "$setOnInsert": {
            "created_at": user_profile.created_at
        }
    };

    collection.update_one(filter, update).await?;

    println!("✅ Successfully saved user profile for: {}", user_profile.username);
    Ok(Json(user_profile))
}

// Update user balance
pub async fn update_user_balance(
    State(state): State<AppState>,
    Json(payload): Json<UpdateBalanceRequest>,
) -> Result<Json<UserProfile>> {
    println!("💰 Updating balance for user: {}", payload.user_id);

    if payload.user_id.is_empty() {
        return Err(AppError::invalid_data("User ID is required"));
    }

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    let filter = doc! { "user_id": &payload.user_id };

    let update = doc! {
        "$set": {
            "balance": payload.balance,
            "updated_at": BsonDateTime::from_chrono(Utc::now())
        }
    };

    let update_result = collection.update_one(filter.clone(), update).await?;

    if update_result.matched_count == 0 {
        println!("❌ User not found: {}", payload.user_id);
        return Err(AppError::DocumentNotFound);
    }

    // Fetch and return updated user
    match collection.find_one(filter).await? {
        Some(user) => {
            println!("✅ Updated balance for {} to: {}", user.username, user.balance);
            Ok(Json(user))
        }
        None => {
            println!("❌ User not found after update: {}", payload.user_id);
            Err(AppError::DocumentNotFound)
        }
    }
}

// Get user statistics
pub async fn get_user_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting user statistics...");

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    // Get all users
    let cursor = collection.find(doc! {}).await?;
    let users: Vec<UserProfile> = cursor.try_collect().await?;

    // Calculate statistics
    let total_users = users.len() as i64;
    let total_balance: f64 = users.iter().map(|u| u.balance).sum();
    let total_bets: i64 = users.iter().map(|u| u.number_of_bets as i64).sum();

    // Find top users by balance
    let mut sorted_users = users.clone();
    sorted_users.sort_by(|a, b| b.balance.partial_cmp(&a.balance).unwrap());

    let top_users: Vec<_> = sorted_users.iter()
        .take(10)
        .map(|u| serde_json::json!({
            "username": u.username,
            "balance": u.balance,
            "bets": u.number_of_bets,
            "club": u.club_fan
        }))
        .collect();

    // Count by club
    use std::collections::HashMap;
    let mut club_counts: HashMap<String, i64> = HashMap::new();

    for user in &users {
        if !user.club_fan.is_empty() {
            *club_counts.entry(user.club_fan.clone()).or_insert(0) += 1;
        }
    }

    let club_stats: Vec<_> = club_counts.into_iter()
        .map(|(club, count)| serde_json::json!({
            "club": club,
            "count": count
        }))
        .collect();

    let stats = serde_json::json!({
        "total_users": total_users,
        "total_balance": total_balance,
        "total_bets": total_bets,
        "average_balance": if total_users > 0 { total_balance / total_users as f64 } else { 0.0 },
        "average_bets": if total_users > 0 { total_bets as f64 / total_users as f64 } else { 0.0 },
        "top_users": top_users,
        "by_club": club_stats,
        "recent_users": users.iter()
            .take(5)
            .map(|u| serde_json::json!({
                "id": u.user_id,
                "username": u.username,
                "phone": u.phone,
                "balance": u.balance,
                "created_at": u.created_at
            }))
            .collect::<Vec<_>>()
    });

    println!("✅ Successfully fetched user statistics");
    Ok(Json(stats))
}

// Get recent users
pub async fn get_recent_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<UserProfile>>> {
    println!("🕒 Getting recent users...");

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    let cursor = collection.find(doc! {}).await?;
    let mut users: Vec<UserProfile> = cursor.try_collect().await?;

    // Sort by created_at descending (most recent first)
    users.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // Take only last 20
    let recent_users: Vec<UserProfile> = users.into_iter().take(20).collect();

    println!("✅ Successfully fetched {} recent users", recent_users.len());
    Ok(Json(recent_users))
}

// Create a new user profile
pub async fn create_user_profile(
    State(state): State<AppState>,
    Json(payload): Json<CreateUserProfile>,
) -> Result<Json<UserProfile>> {
    println!("🎯 Creating new user profile: {}", payload.username);

    // Validate the request
    if let Err(validation_errors) = payload.validate() {
        return Err(AppError::invalid_data(&format!("Validation failed: {:?}", validation_errors)));
    }

    let collection: Collection<UserProfile> = state.db.collection("user_profiles");

    // Check if user already exists
    let existing_filter = doc! { 
        "$or": [
            { "user_id": &payload.user_id },
            { "phone": &payload.phone }
        ]
    };

    if let Some(existing) = collection.find_one(existing_filter).await? {
        println!("⚠️ User already exists: {}", existing.username);
        return Err(AppError::invalid_data("User with this ID or phone already exists"));
    }

    let now = Utc::now();
    let user_profile = UserProfile {
        id: Some(ObjectId::new()),
        user_id: payload.user_id,
        username: payload.username,
        phone: payload.phone,
        nickname: payload.nickname,
        club_fan: payload.club_fan,
        country_fan: payload.country_fan,
        balance: payload.balance,
        number_of_bets: payload.number_of_bets,
        created_at: BsonDateTime::from_chrono(now),
        updated_at: BsonDateTime::from_chrono(now),
    };

    // Insert the user
    collection.insert_one(&user_profile).await?;

    println!("✅ Successfully created user profile: {}", user_profile.username);
    Ok(Json(user_profile))
}