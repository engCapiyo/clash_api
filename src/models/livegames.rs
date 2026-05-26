use bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};

// Main Game model - matches your database exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub match_id: String,
    pub home_team: String,
    pub away_team: String,
    pub league: String,
    pub home_win: f64,
    pub away_win: f64,
    pub draw: f64,
    pub date: String,
    pub time: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub status: String,
    pub is_live: bool,
    pub last_updated: BsonDateTime,
    pub created_at: Option<BsonDateTime>,
    pub scraped_at: Option<BsonDateTime>,
}

// For live game score updates
#[derive(Debug, Deserialize)]
pub struct LiveUpdate {
    pub match_id: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub minute: Option<String>,
    pub assist: Option<String>, // <-- ONLY ADDED THIS
}

// API response for live games
#[derive(Debug, Serialize)]
pub struct LiveGamesResponse {
    pub live_games: Vec<Game>,
    pub count: usize,
    pub last_updated: BsonDateTime,
}

// Query parameters for live games
#[derive(Debug, Deserialize)]
pub struct LiveQuery {
    pub league: Option<String>,
    pub limit: Option<u32>,
}
