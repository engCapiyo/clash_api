use axum::{
    extract::{Path, State},
    response::Json,
};
use mongodb::bson::{doc, to_bson, DateTime as BsonDateTime};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing;

use crate::errors::{AppError, Result};
use crate::models::line_up::{LineupsDocument, Player};
use crate::state::AppState;

// ============================================================================
// REQUEST STRUCTS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RawPlayer {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: bool,
    #[serde(rename = "playerId")]
    pub player_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawCoach {
    pub name: String,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawTeamLineup {
    pub formation: String,
    pub players: Vec<RawPlayer>,
    pub bench: Vec<RawPlayer>,
    pub coach: RawCoach,
}

#[derive(Debug, Deserialize)]
pub struct RawLineupsData {
    pub home: RawTeamLineup,
    pub away: RawTeamLineup,
}

#[derive(Debug, Deserialize)]
pub struct LineupsUpdateRequest {
    pub fixture_id: String,
    pub lineups: RawLineupsData,
    pub timestamp: String,
}

// ============================================================================
// RECEIVE LINEUPS FROM POLLER
// ============================================================================

pub async fn receive_lineups_update(
    State(state): State<AppState>,
    Json(update): Json<LineupsUpdateRequest>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📋 Receiving lineups for fixture: {}", update.fixture_id);

    // Fetch fixture details from games collection to get team names
    let games_col: Collection<serde_json::Value> = state.db.collection("games");
    let filter = doc! { "match_id": &update.fixture_id };

    let fixture = games_col.find_one(filter).await?;
    let (home_team, away_team) = match fixture {
        Some(game) => (
            game.get("home_team")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string(),
            game.get("away_team")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string(),
        ),
        None => {
            tracing::warn!("Fixture not found: {}", update.fixture_id);
            ("Unknown".to_string(), "Unknown".to_string())
        }
    };

    // Convert to Player structs - Starting XI
    let home_starting_xi: Vec<Player> = update
        .lineups
        .home
        .players
        .iter()
        .filter(|p| p.lineup)
        .map(|p| Player {
            name: p.name.clone(),
            position: p.position.clone(),
            jersey_number: p.jersey_number,
            captain: p.captain,
            lineup: p.lineup,
            player_id: p.player_id.clone(),
            rating: None,
        })
        .collect();

    // Convert to Player structs - Bench
    let home_bench: Vec<Player> = update
        .lineups
        .home
        .bench
        .iter()
        .map(|p| Player {
            name: p.name.clone(),
            position: p.position.clone(),
            jersey_number: p.jersey_number,
            captain: false,
            lineup: false,
            player_id: p.player_id.clone(),
            rating: None,
        })
        .collect();

    // Away team starting XI
    let away_starting_xi: Vec<Player> = update
        .lineups
        .away
        .players
        .iter()
        .filter(|p| p.lineup)
        .map(|p| Player {
            name: p.name.clone(),
            position: p.position.clone(),
            jersey_number: p.jersey_number,
            captain: p.captain,
            lineup: p.lineup,
            player_id: p.player_id.clone(),
            rating: None,
        })
        .collect();

    // Away team bench
    let away_bench: Vec<Player> = update
        .lineups
        .away
        .bench
        .iter()
        .map(|p| Player {
            name: p.name.clone(),
            position: p.position.clone(),
            jersey_number: p.jersey_number,
            captain: false,
            lineup: false,
            player_id: p.player_id.clone(),
            rating: None,
        })
        .collect();

    // Create lineups document
    let lineup_doc = LineupsDocument {
        id: format!("lineup_{}", update.fixture_id),
        match_id: update.fixture_id.clone(),
        home_team: home_team.clone(),
        away_team: away_team.clone(),
        home_formation: update.lineups.home.formation.clone(),
        away_formation: update.lineups.away.formation.clone(),
        home_coach: update.lineups.home.coach.name.clone(),
        away_coach: update.lineups.away.coach.name.clone(),
        home_starting_xi,
        home_bench,
        away_starting_xi,
        away_bench,
        fetched_at: BsonDateTime::from_chrono(chrono::Utc::now()),
    };

    // Store in database
    let lineups_col: Collection<LineupsDocument> = state.db.collection("lineups");

    // Upsert (update if exists, insert if not)
    let filter = doc! { "match_id": &update.fixture_id };

    // Convert to BSON with proper error handling
    let bson_doc = to_bson(&lineup_doc).map_err(|e| {
        AppError::InternalServerError(format!("Failed to serialize lineups: {}", e))
    })?;

    let update_doc = doc! { "$set": bson_doc };

    lineups_col
        .update_one(filter, update_doc)
        .upsert(true)
        .await?;

    // Also update games collection with lineups available flag
    games_col
        .update_one(
            doc! { "match_id": &update.fixture_id },
            doc! { "$set": { "lineups_available": true, "lineups_fetched_at": BsonDateTime::from_chrono(chrono::Utc::now()) } },
        )
        .await?;

    tracing::info!("✅ Lineups stored for {} vs {}", home_team, away_team);

    Ok(Json(json!({
        "success": true,
        "message": "Lineups stored successfully",
        "fixture_id": update.fixture_id,
    })))
}

// ============================================================================
// GET LINEUPS FOR A MATCH
// ============================================================================

pub async fn get_lineups(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📋 GET /api/games/{}/lineups called", match_id);

    let collection: Collection<LineupsDocument> = state.db.collection("lineups");
    let filter = doc! { "match_id": &match_id };

    match collection.find_one(filter).await? {
        Some(lineups) => Ok(Json(json!({
            "success": true,
            "data": lineups,
        }))),
        None => Ok(Json(json!({
            "success": false,
            "message": "Lineups not available yet",
            "data": null,
        }))),
    }
}

// ============================================================================
// GET SIMPLIFIED LINEUPS (for quick display)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SimplifiedPlayer {
    pub name: String,
    pub position: String,
    pub number: i32,
    pub captain: bool,
}

#[derive(Debug, Serialize)]
pub struct SimplifiedLineups {
    pub home_formation: String,
    pub away_formation: String,
    pub home_coach: String,
    pub away_coach: String,
    pub home_starting_xi: Vec<SimplifiedPlayer>,
    pub away_starting_xi: Vec<SimplifiedPlayer>,
}

impl From<&Player> for SimplifiedPlayer {
    fn from(player: &Player) -> Self {
        Self {
            name: player.name.clone(),
            position: player.position.clone(),
            number: player.jersey_number,
            captain: player.captain,
        }
    }
}

pub async fn get_simplified_lineups(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("📋 GET /api/games/{}/lineups/simplified called", match_id);

    let collection: Collection<LineupsDocument> = state.db.collection("lineups");
    let filter = doc! { "match_id": &match_id };

    match collection.find_one(filter).await? {
        Some(lineups) => {
            let simplified = SimplifiedLineups {
                home_formation: lineups.home_formation,
                away_formation: lineups.away_formation,
                home_coach: lineups.home_coach,
                away_coach: lineups.away_coach,
                home_starting_xi: lineups
                    .home_starting_xi
                    .iter()
                    .map(SimplifiedPlayer::from)
                    .collect(),
                away_starting_xi: lineups
                    .away_starting_xi
                    .iter()
                    .map(SimplifiedPlayer::from)
                    .collect(),
            };

            Ok(Json(json!({
                "success": true,
                "data": simplified,
            })))
        }
        None => Ok(Json(json!({
            "success": false,
            "message": "Lineups not available yet",
            "data": null,
        }))),
    }
}

// ============================================================================
// CHECK IF LINEUPS ARE AVAILABLE
// ============================================================================

pub async fn check_lineups_available(
    State(state): State<AppState>,
    Path(match_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!("🔍 Checking if lineups available for match: {}", match_id);

    let games_col: Collection<serde_json::Value> = state.db.collection("games");
    let filter = doc! { "match_id": &match_id };

    match games_col.find_one(filter).await? {
        Some(game) => {
            let available = game
                .get("lineups_available")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            Ok(Json(json!({
                "success": true,
                "available": available,
                "fixture_id": match_id,
            })))
        }
        None => Ok(Json(json!({
            "success": false,
            "available": false,
            "message": "Fixture not found",
        }))),
    }
}
