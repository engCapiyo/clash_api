// handlers/visibility_handler.rs

use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::Utc;
use mongodb::bson::{doc, DateTime as BsonDateTime};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::errors::{AppError, Result};
use crate::models::votes_visibility::VotesVisibility;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateVisibilityRequest {
    pub key: String,
    pub value: bool,
}

#[derive(Debug, Serialize)]
pub struct VisibilityResponse {
    pub key: String,
    pub value: bool,
}

// GET /visibility/{key}
pub async fn get_visibility(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<VisibilityResponse>> {
    println!("🔍 GET /visibility/{}", key);

    let collection: Collection<VotesVisibility> = state.db.collection("visibility");
    let filter = doc! { "key": &key };

    match collection.find_one(filter).await? {
        Some(doc) => {
            println!("✅ Found visibility for key: {}", key);
            Ok(Json(VisibilityResponse {
                key: doc.key,
                value: doc.value,
            }))
        }
        None => {
            println!(
                "ℹ️ No visibility setting found for key: {}, returning default true",
                key
            );
            Ok(Json(VisibilityResponse {
                key: key,
                value: true, // default
            }))
        }
    }
}

// POST /visibility
// handlers/visibility_handler.rs

// POST /visibility - Create or update
pub async fn update_visibility(
    State(state): State<AppState>,
    Json(payload): Json<UpdateVisibilityRequest>,
) -> Result<Json<VisibilityResponse>> {
    let collection: Collection<VotesVisibility> = state.db.collection("visibility");

    // Check if exists
    let filter = doc! { "key": &payload.key };
    let existing = collection.find_one(filter.clone()).await?;

    let now = Utc::now();

    let doc = if let Some(mut existing_doc) = existing {
        // Update existing
        existing_doc.value = payload.value;
        existing_doc.updated_at = now;
        existing_doc
    } else {
        // Create new
        VotesVisibility {
            id: None,
            key: payload.key.clone(),
            value: payload.value,
            created_at: now,
            description: "Show/hide votes button globally".to_string(),
            updated_at: now,
        }
    };

    // Upsert
    let update = doc! {
        "$set": {
            "value": payload.value,
            "updated_at": BsonDateTime::from_chrono(now),
        }
    };
    collection.update_one(filter, update).await?;

    Ok(Json(VisibilityResponse {
        key: payload.key,
        value: payload.value,
    }))
}

// GET /visibility/batch?keys=key1,key2,key3
pub async fn get_batch_visibility(
    State(state): State<AppState>,
    Query(params): Query<BatchVisibilityRequest>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 GET /visibility/batch with keys: {:?}", params.keys);

    let collection: Collection<VotesVisibility> = state.db.collection("visibility");

    let keys: Vec<String> = params
        .keys
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if keys.is_empty() {
        return Ok(Json(serde_json::json!({})));
    }

    let filter = doc! { "key": { "$in": &keys } };
    let cursor = collection.find(filter).await?;

    let mut results = serde_json::Map::new();

    // Get all existing visibility settings
    use futures_util::TryStreamExt;
    let docs: Vec<VotesVisibility> = cursor.try_collect().await?;

    // Create a map of key -> value
    let mut map = std::collections::HashMap::new();
    for doc in docs {
        map.insert(doc.key, doc.value);
    }

    // Build response with defaults
    for key in keys {
        let value = map.get(&key).copied().unwrap_or(true);
        results.insert(key.clone(), serde_json::json!(value));
    }

    println!("✅ Successfully fetched batch visibility");

    Ok(Json(serde_json::Value::Object(results)))
}

#[derive(Debug, Deserialize)]
pub struct BatchVisibilityRequest {
    pub keys: String,
}
