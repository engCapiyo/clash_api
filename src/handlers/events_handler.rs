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
use crate::models::events::{Event, EventRequest, EventsResponse, SingleEventResponse};
use crate::state::AppState;

// ============================================================================
// GET all events for a match (sorted by minute)
// ============================================================================

pub async fn get_match_events(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<EventsResponse>> {
    tracing::info!("📜 GET /api/games/{}/events called", match_id);

    let collection: Collection<Event> = state.db.collection("events");
    let filter = doc! { "match_id": &match_id };
    let sort = doc! { "minute": 1 };

    let cursor = collection.find(filter).sort(sort).await?;
    let events: Vec<Event> = cursor.try_collect().await?;

    let count = events.len(); // <-- Get count BEFORE moving events

    Ok(Json(EventsResponse {
        success: true,
        data: events, // events moved here
        count,        // use the pre-calculated count
    }))
}

pub async fn get_events_by_type(
    State(state): State<AppState>,
    Path((match_id, event_type)): Path<(String, String)>,
) -> Result<Json<EventsResponse>> {
    tracing::info!(
        "📜 GET /api/games/{}/events/{} called",
        match_id,
        event_type
    );

    let collection: Collection<Event> = state.db.collection("events");
    let filter = doc! {
        "match_id": &match_id,
        "event_type": &event_type
    };
    let sort = doc! { "minute": 1 };

    let cursor = collection.find(filter).sort(sort).await?;
    let events: Vec<Event> = cursor.try_collect().await?;

    let count = events.len(); // Get count before moving

    tracing::info!("✅ Fetched {} {} events", count, event_type);

    Ok(Json(EventsResponse {
        success: true,
        data: events,
        count,
    }))
}
// ============================================================================
// GET events by type (goals, cards, etc.)
// ============================================================================

// ============================================================================
// GET latest event in a match
// ============================================================================
pub async fn get_latest_event(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<SingleEventResponse>> {
    tracing::info!("⚡ GET /api/games/{}/events/latest called", match_id);

    let collection: Collection<Event> = state.db.collection("events");
    let filter = doc! { "match_id": &match_id };
    let sort = doc! { "minute": -1 };

    let event = collection.find_one(filter).sort(sort).await?;

    Ok(Json(SingleEventResponse {
        success: event.is_some(),
        data: event,
    }))
}

// ============================================================================
// ADD event from Python poller
// ============================================================================
pub async fn add_event(
    State(state): State<AppState>,
    Json(req): Json<EventRequest>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        "➕ Adding event: {} for match {} at minute {}",
        req.event_type,
        req.match_id,
        req.minute
    );

    let event = Event::from_request(req);

    let collection: Collection<Event> = state.db.collection("events");
    collection.insert_one(&event).await?;

    tracing::info!("✅ Event added successfully");

    Ok(Json(json!({
        "success": true,
        "message": "Event added successfully",
        "event_id": event.id,
    })))
}

// ============================================================================
// ADD multiple events in bulk (for backfill)
// ============================================================================
pub async fn bulk_add_events(
    State(state): State<AppState>,
    Json(requests): Json<Vec<EventRequest>>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📦 Bulk adding {} events", requests.len());

    let collection: Collection<Event> = state.db.collection("events");

    let events: Vec<Event> = requests.into_iter().map(Event::from_request).collect();

    let result = collection.insert_many(events).await?;

    tracing::info!("✅ Bulk added {} events", result.inserted_ids.len());

    Ok(Json(json!({
        "success": true,
        "message": format!("Added {} events", result.inserted_ids.len()),
        "inserted_count": result.inserted_ids.len(),
    })))
}

// ============================================================================
// DELETE all events for a match (cleanup)
// ============================================================================
pub async fn delete_match_events(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🗑️ Deleting all events for match {}", match_id);

    let collection: Collection<Event> = state.db.collection("events");
    let filter = doc! { "match_id": &match_id };
    let result = collection.delete_many(filter).await?;

    tracing::info!("✅ Deleted {} events", result.deleted_count);

    Ok(Json(json!({
        "success": true,
        "message": format!("Deleted {} events", result.deleted_count),
        "deleted_count": result.deleted_count,
    })))
}

// ============================================================================
// DELETE specific event by ID
// ============================================================================
pub async fn delete_event_by_id(
    State(state): State<AppState>,
    Path(event_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🗑️ Deleting event with id: {}", event_id);

    let collection: Collection<Event> = state.db.collection("events");
    let filter = doc! { "_id": event_id };
    let result = collection.delete_one(filter).await?;

    if result.deleted_count == 0 {
        return Ok(Json(json!({
            "success": false,
            "message": "Event not found",
        })));
    }

    Ok(Json(json!({
        "success": true,
        "message": "Event deleted successfully",
    })))
}
