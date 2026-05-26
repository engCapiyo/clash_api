use axum::{
    extract::{Path, State},
    response::Json,
    Json as AxumJson,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId, DateTime as BsonDateTime},
    Collection,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    errors::{AppError, Result},
    state::AppState,
};

// ============================================================================
// MODELS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comrade {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub comrade_id: String,
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
    pub status: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AddComradeRequest {
    pub user_id: String,
    pub comrade_id: String,
    pub username: String,
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoveComradeRequest {
    pub user_id: String,
    pub comrade_id: String,
}

#[derive(Debug, Serialize)]
pub struct ComradeResponse {
    pub id: String,
    pub comrade_id: String,
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub is_mutual: bool,
}

// ============================================================================
// ADD COMRADE WITH FCM NOTIFICATION
// ============================================================================

pub async fn add_comrade(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<AddComradeRequest>,
) -> Result<Json<ComradeResponse>> {
    println!("\n🎯 ========== ADDING COMRADE ==========");
    println!(
        "🎯 User {} adding comrade {}",
        payload.username, payload.comrade_username
    );

    // Validate required fields
    if payload.user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }
    if payload.comrade_id.is_empty() {
        return Err(AppError::missing_field("comrade_id"));
    }
    if payload.username.is_empty() {
        return Err(AppError::missing_field("username"));
    }
    if payload.comrade_username.is_empty() {
        return Err(AppError::missing_field("comrade_username"));
    }

    let collection: Collection<Comrade> = state.db.collection("comrades");
    let now = Utc::now();

    // Check if relationship already exists
    let existing = collection
        .find_one(doc! {
            "user_id": &payload.user_id,
            "comrade_id": &payload.comrade_id,
        })
        .await?;

    if existing.is_some() {
        println!("⚠️ Comrade relationship already exists");
        return Err(AppError::UserAlreadyExists);
    }

    // Create record for USER -> COMRADE
    let user_to_comrade = Comrade {
        id: Some(ObjectId::new()),
        user_id: payload.user_id.clone(),
        comrade_id: payload.comrade_id.clone(),
        comrade_username: payload.comrade_username.clone(),
        comrade_nickname: payload.comrade_nickname.clone(),
        comrade_club: payload.comrade_club.clone(),
        comrade_country: payload.comrade_country.clone(),
        status: "active".to_string(),
        created_at: now,
    };

    // Create record for COMRADE -> USER (AUTOMATIC MUTUAL)
    let comrade_to_user = Comrade {
        id: Some(ObjectId::new()),
        user_id: payload.comrade_id.clone(),
        comrade_id: payload.user_id.clone(),
        comrade_username: payload.username.clone(),
        comrade_nickname: payload.username.clone(),
        comrade_club: payload.comrade_club.clone(),
        comrade_country: payload.comrade_country.clone(),
        status: "active".to_string(),
        created_at: now,
    };

    // Insert both records
    collection.insert_one(&user_to_comrade).await?;
    collection.insert_one(&comrade_to_user).await?;
    println!("✅ Both comrade records inserted");

    // ============================================
    // SEND FCM NOTIFICATION TO THE COMRADE
    // ============================================
    println!(
        "📱 Sending FCM notification to comrade: {}",
        payload.comrade_username
    );

    if let Some(fcm_service) = &state.fcm_service {
        let notification_title = format!("New Comrade! 🎉");
        let notification_body = format!("{} added you as a comrade", payload.username);

        let notification_data = json!({
            "type": "comrade_added",
            "user_id": payload.user_id,
            "username": payload.username,
            "comrade_id": payload.comrade_id,
            "comrade_username": payload.comrade_username,
            "timestamp": now.to_rfc3339(),
        });

        // Send notification to the comrade
        match fcm_service
            .send_to_user(
                &state,
                &payload.comrade_id,
                &notification_title,
                &notification_body,
                notification_data,
                "comrade_added",
            )
            .await
        {
            Ok(sent) => {
                if sent {
                    println!("✅ FCM notification sent to {}", payload.comrade_username);
                } else {
                    println!("⚠️ No FCM token found for {}", payload.comrade_username);
                }
            }
            Err(e) => {
                println!("❌ Failed to send FCM notification: {}", e);
                // Don't fail the request if notification fails
            }
        }
    } else {
        println!("⚠️ FCM service not initialized, skipping notification");
    }

    let response = ComradeResponse {
        id: user_to_comrade.id.unwrap().to_hex(),
        comrade_id: payload.comrade_id,
        comrade_username: payload.comrade_username,
        comrade_nickname: payload.comrade_nickname,
        comrade_club: payload.comrade_club,
        comrade_country: payload.comrade_country,
        status: "active".to_string(),
        created_at: now,
        is_mutual: true, // Always true now
    };

    println!("✅ Mutual comrade relationship created!");
    println!("🎯 ========== ADD COMRADE COMPLETE ==========\n");
    Ok(Json(response))
}

// ============================================================================
// REMOVE COMRADE (TWO-WAY REMOVAL)
// ============================================================================

pub async fn remove_comrade(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<RemoveComradeRequest>,
) -> Result<Json<serde_json::Value>> {
    println!("\n🗑️ ========== REMOVING COMRADE ==========");
    println!(
        "🗑️ User {} removing comrade {}",
        payload.user_id, payload.comrade_id
    );

    if payload.user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }
    if payload.comrade_id.is_empty() {
        return Err(AppError::missing_field("comrade_id"));
    }

    let collection: Collection<Comrade> = state.db.collection("comrades");

    // Delete both directions
    let result1 = collection
        .delete_one(doc! {
            "user_id": &payload.user_id,
            "comrade_id": &payload.comrade_id,
        })
        .await?;

    let result2 = collection
        .delete_one(doc! {
            "user_id": &payload.comrade_id,
            "comrade_id": &payload.user_id,
        })
        .await?;

    let total_deleted = result1.deleted_count + result2.deleted_count;

    if total_deleted == 0 {
        return Err(AppError::DocumentNotFound);
    }

    // Send notification about removal
    if let Some(fcm_service) = &state.fcm_service {
        // Get usernames first (you might want to fetch from database)
        let notification_title = "Comrade Removed".to_string();
        let notification_body = format!("A comrade relationship has ended");

        let _ = fcm_service
            .send_to_user(
                &state,
                &payload.comrade_id,
                &notification_title,
                &notification_body,
                json!({
                    "type": "comrade_removed",
                    "user_id": payload.user_id,
                }),
                "comrade_removed",
            )
            .await;
    }

    println!("✅ Removed {} comrade records", total_deleted);
    println!("🗑️ ========== REMOVE COMRADE COMPLETE ==========\n");

    Ok(Json(json!({
        "success": true,
        "message": "Comrade removed successfully",
        "deleted_count": total_deleted
    })))
}

// ============================================================================
// GET USER'S COMRADES
// ============================================================================

pub async fn get_user_comrades(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<ComradeResponse>>> {
    println!("🔍 Getting comrades for user: {}", user_id);

    if user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }

    let collection: Collection<Comrade> = state.db.collection("comrades");

    let filter = doc! { "user_id": &user_id };
    let cursor = collection.find(filter).await?;
    let comrades: Vec<Comrade> = cursor.try_collect().await?;

    let responses: Vec<ComradeResponse> = comrades
        .into_iter()
        .map(|comrade| {
            // With mutual addition, all relationships are mutual
            ComradeResponse {
                id: comrade.id.unwrap().to_hex(),
                comrade_id: comrade.comrade_id,
                comrade_username: comrade.comrade_username,
                comrade_nickname: comrade.comrade_nickname,
                comrade_club: comrade.comrade_club,
                comrade_country: comrade.comrade_country,
                status: comrade.status,
                created_at: comrade.created_at,
                is_mutual: true, // Always true in mutual model
            }
        })
        .collect();

    println!("✅ Found {} comrades for user", responses.len());
    Ok(Json(responses))
}

// ============================================================================
// GET COMRADE STATS
// ============================================================================

pub async fn get_comrade_stats(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting comrade stats for user: {}", user_id);

    if user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }

    let collection: Collection<Comrade> = state.db.collection("comrades");

    let count = collection
        .count_documents(doc! { "user_id": &user_id })
        .await?;

    let max_comrades = 50; // Default max
    let remaining = (max_comrades - count as i32).max(0);
    let can_add_more = count < max_comrades as u64;

    let response = json!({
        "count": count,
        "max_comrades": max_comrades,
        "remaining": remaining,
        "can_add_more": can_add_more,
    });

    println!("✅ Comrade stats: {}/{}", count, max_comrades);
    Ok(Json(response))
}

// ============================================================================
// UPGRADE COMRADE LIMIT (PAID TIER)
// ============================================================================

pub async fn upgrade_comrade_limit(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    println!("💎 Upgrading comrade limit for user: {}", user_id);

    if user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }

    // Get users collection (or user_profiles, depending on your schema)
    let users_collection: Collection<serde_json::Value> = state.db.collection("users");

    // Update the user's max_comrades limit
    // Default is 50, upgrade to 200 (or 500 for premium)
    let new_limit = 200;

    let result = users_collection
        .update_one(
            doc! { "user_id": &user_id }, // or "_id": &user_id
            doc! {
                "$set": {
                    "max_comrades": new_limit,
                    "comrade_tier": "premium",
                    "upgraded_at": BsonDateTime::now()
                }
            },
        )
        .await
        .map_err(|e| AppError::InternalServerError(format!("Database error: {}", e)))?;

    if result.matched_count == 0 {
        // Try updating in user_profiles collection if not found in users
        let profiles_collection: Collection<serde_json::Value> =
            state.db.collection("user_profiles");

        let profile_result = profiles_collection
            .update_one(
                doc! { "user_id": &user_id },
                doc! {
                    "$set": {
                        "max_comrades": new_limit,
                        "comrade_tier": "premium",
                        "upgraded_at": BsonDateTime::now()
                    }
                },
            )
            .await
            .map_err(|e| AppError::InternalServerError(format!("Database error: {}", e)))?;

        if profile_result.matched_count == 0 {
            return Err(AppError::DocumentNotFound);
        }
    }

    println!(
        "✅ Comrade limit upgraded to {} for user: {}",
        new_limit, user_id
    );

    Ok(Json(json!({
        "success": true,
        "message": format!("Comrade limit upgraded to {}", new_limit),
        "user_id": user_id,
        "max_comrades": new_limit,
        "tier": "premium"
    })))
}

// ============================================================================
// GET AVAILABLE USERS (USERS NOT YET COMRADES)
// ============================================================================

pub async fn get_available_users(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>> {
    println!("🔍 Getting available users for user: {}", user_id);

    if user_id.is_empty() {
        return Err(AppError::missing_field("user_id"));
    }

    let profiles_collection: Collection<serde_json::Value> = state.db.collection("user_profiles");
    let comrades_collection: Collection<Comrade> = state.db.collection("comrades");

    // Get existing comrades (both directions since mutual)
    let existing_comrades: Vec<String> = comrades_collection
        .find(doc! {
            "$or": vec![
                doc! { "user_id": &user_id },
                doc! { "comrade_id": &user_id }
            ]
        })
        .await?
        .try_collect::<Vec<Comrade>>()
        .await?
        .into_iter()
        .flat_map(|c| vec![c.user_id, c.comrade_id])
        .filter(|id| id != &user_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut users = Vec::new();
    let mut cursor = profiles_collection.find(doc! {}).await?;

    while let Some(profile) = cursor.try_next().await? {
        let profile_user_id = profile["user_id"].as_str().unwrap_or("").to_string();

        // Skip current user and existing comrades
        if profile_user_id == user_id || existing_comrades.contains(&profile_user_id) {
            continue;
        }

        users.push(json!({
            "user_id": profile_user_id,
            "username": profile["username"].as_str().unwrap_or(""),
            "nickname": profile["nickname"].as_str().unwrap_or(""),
            "club_fan": profile["club_fan"].as_str().unwrap_or(""),
            "country_fan": profile["country_fan"].as_str().unwrap_or(""),
            "is_already_comrade": false,
        }));
    }

    println!("✅ Found {} available users", users.len());
    Ok(Json(users))
}
