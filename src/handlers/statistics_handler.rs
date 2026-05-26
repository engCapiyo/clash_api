use axum::{
    extract::{Path, State},
    response::Json,
};
use futures_util::TryStreamExt;
use mongodb::bson::doc;
use mongodb::Collection;
use serde_json::json;
use tracing;

use crate::errors::Result;
use crate::models::statistics::{MatchStatistics, StatisticsRequest};
use crate::state::AppState;

// GET all statistics for a match
pub async fn get_match_statistics(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<MatchStatistics> = state.db.collection("statistics");
    let filter = doc! { "match_id": &match_id };
    let sort = doc! { "minute": 1 };

    let cursor = collection.find(filter).sort(sort).await?;
    let stats: Vec<MatchStatistics> = cursor.try_collect().await?;

    Ok(Json(json!({
        "success": true,
        "data": stats,
        "count": stats.len(),
    })))
}

// GET latest statistics
pub async fn get_latest_statistics(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<MatchStatistics> = state.db.collection("statistics");
    let filter = doc! { "match_id": &match_id };
    let sort = doc! { "minute": -1 };

    let stats = collection.find_one(filter).sort(sort).await?;

    match stats {
        Some(s) => Ok(Json(json!({
            "success": true,
            "data": s,
        }))),
        None => Ok(Json(json!({
            "success": false,
            "message": "No statistics available",
            "data": null,
        }))),
    }
}

// GET statistics at specific minute
pub async fn get_statistics_at_minute(
    State(state): State<AppState>,
    Path((match_id, minute)): Path<(String, i32)>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<MatchStatistics> = state.db.collection("statistics");
    let filter = doc! {
        "match_id": &match_id,
        "minute": minute
    };

    let stats = collection.find_one(filter).await?;

    match stats {
        Some(s) => Ok(Json(json!({
            "success": true,
            "data": s,
        }))),
        None => Ok(Json(json!({
            "success": false,
            "message": format!("No statistics found for minute {}", minute),
            "data": null,
        }))),
    }
}

// ADD statistics snapshot from poller
pub async fn add_statistics_snapshot(
    State(state): State<AppState>,
    Json(req): Json<StatisticsRequest>,
) -> Result<Json<serde_json::Value>> {
    let stats = MatchStatistics::from_request(req);

    let collection: Collection<MatchStatistics> = state.db.collection("statistics");
    collection.insert_one(&stats).await?;

    Ok(Json(json!({
        "success": true,
        "message": "Statistics snapshot added",
    })))
}

// BULK add statistics
pub async fn bulk_update_statistics(
    State(state): State<AppState>,
    Json(requests): Json<Vec<StatisticsRequest>>,
) -> Result<Json<serde_json::Value>> {
    let collection: Collection<MatchStatistics> = state.db.collection("statistics");

    let mut inserted = 0;
    for req in requests {
        let stats = MatchStatistics::from_request(req);
        collection.insert_one(&stats).await?;
        inserted += 1;
    }

    Ok(Json(json!({
        "success": true,
        "inserted": inserted,
    })))
}
