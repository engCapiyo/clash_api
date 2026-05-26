use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use mongodb::bson::{oid::ObjectId, serde_helpers};

// Model for creating a new bet (when user accepts a pledge)
#[derive(Debug, Deserialize)]
pub struct CreateBetRequest {
    // Pledge info
    #[serde(rename = "pledge_id")]
    pub pledge_id: PledgeId,

    // Starter info (original bet creator)
    pub starter_id: String,
    pub starter_username: String,
    pub starter_selection: String, // "home_team", "away_team", or "draw"
    #[serde(deserialize_with = "deserialize_f64")]
    pub starter_amount: f64,
    pub starter_team: String,

    // Finisher info (user accepting the bet)
    pub finisher_id: String,
    pub finisher_username: String,
    pub finisher_selection: String,
    #[serde(default)]
    pub finisher_amount: Option<f64>, // Can be calculated if not provided
    pub finisher_team: String,

    // Match info
    pub home_team: String,
    pub away_team: String,
    pub match_time: Option<DateTime<Utc>>,
    pub league: String,
    pub sport_type: String,

    // Bet details
    #[serde(deserialize_with = "deserialize_f64")]
    pub total_pot: f64,
    #[serde(default = "default_status")]
    pub status: String, // "active", "completed", "cancelled"

    // Odds
    pub odds: BetOdds,

    // Optional fields that might not be in incoming JSON
    #[serde(default)]
    pub winner_id: Option<String>,
    #[serde(default)]
    pub winner_username: Option<String>,
    #[serde(default)]
    pub winning_selection: Option<String>,
}

// Bet odds structure
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BetOdds {
    pub home_win: String,
    pub away_win: String,
    pub draw: String,
}

// Handle MongoDB ObjectId in JSON (both string and object formats)
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PledgeId {
    String(String),
    ObjectId { #[serde(rename = "$oid")] oid: String },
}

impl PledgeId {
    pub fn to_string(&self) -> String {
        match self {
            PledgeId::String(s) => s.clone(),
            PledgeId::ObjectId { oid } => oid.clone(),
        }
    }
}

// Database model for Bets collection (MongoDB)
#[derive(Debug, Serialize, Deserialize)]
pub struct Bet {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    // Pledge reference
    pub pledge_id: String,

    // Starter info
    pub starter_id: String,
    pub starter_username: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub starter_team: String,

    // Finisher info
    pub finisher_id: String,
    pub finisher_username: String,
    pub finisher_selection: String,
    pub finisher_amount: f64, // Required in DB, calculated if missing
    pub finisher_team: String,

    // Match info
    pub home_team: String,
    pub away_team: String,
    pub match_time: Option<DateTime<Utc>>,
    pub league: String,
    pub sport_type: String,

    // Bet details
    pub total_pot: f64,
    pub status: String, // "active", "completed", "cancelled"

    // Winner info (filled when match completes)
    pub winner_id: Option<String>,
    pub winner_username: Option<String>,
    pub winning_selection: Option<String>, // "home_win", "away_win", "draw"

    // Odds
    pub odds: BetOdds,

    // Timestamps - removed the problematic serde helpers
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

// Model for updating bet status (when match completes)
#[derive(Debug, Deserialize)]
pub struct UpdateBetRequest {
    pub bet_id: String, // ObjectId as string
    pub winner_id: String,
    pub winner_username: String,
    pub winning_selection: String, // "home_win", "away_win", or "draw"
    pub status: String, // "completed"
}

// Model for bet response
#[derive(Debug, Serialize)]
pub struct BetResponse {
    pub id: String, // ObjectId as string
    pub pledge_id: String,

    pub starter_id: String,
    pub starter_username: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub starter_team: String,

    pub finisher_id: String,
    pub finisher_username: String,
    pub finisher_selection: String,
    pub finisher_amount: f64,
    pub finisher_team: String,

    pub home_team: String,
    pub away_team: String,
    pub match_time: Option<DateTime<Utc>>,
    pub league: String,
    pub sport_type: String,

    pub total_pot: f64,
    pub status: String,

    pub winner_id: Option<String>,
    pub winner_username: Option<String>,
    pub winning_selection: Option<String>,

    pub odds: BetOdds,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

// Model for user balance update
#[derive(Debug, Deserialize)]
pub struct UpdateBalanceRequest {
    pub user_id: String,
    pub balance: f64,
}

// Model for updating pledge status
#[derive(Debug, Deserialize)]
pub struct UpdatePledgeStatusRequest {
    pub status: String, // "matched", "completed", "cancelled"
}

// Helper function to convert Bet to BetResponse
impl From<Bet> for BetResponse {
    fn from(bet: Bet) -> Self {
        BetResponse {
            id: bet.id.map(|id| id.to_hex()).unwrap_or_default(),
            pledge_id: bet.pledge_id,
            starter_id: bet.starter_id,
            starter_username: bet.starter_username,
            starter_selection: bet.starter_selection,
            starter_amount: bet.starter_amount,
            starter_team: bet.starter_team,
            finisher_id: bet.finisher_id,
            finisher_username: bet.finisher_username,
            finisher_selection: bet.finisher_selection,
            finisher_amount: bet.finisher_amount,
            finisher_team: bet.finisher_team,
            home_team: bet.home_team,
            away_team: bet.away_team,
            match_time: bet.match_time,
            league: bet.league,
            sport_type: bet.sport_type,
            total_pot: bet.total_pot,
            status: bet.status,
            winner_id: bet.winner_id,
            winner_username: bet.winner_username,
            winning_selection: bet.winning_selection,
            odds: bet.odds,
            created_at: bet.created_at,
            updated_at: bet.updated_at,
            completed_at: bet.completed_at,
        }
    }
}

// Error response model
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

// Success response model
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

// Helper function to deserialize numbers to f64 (accepts both integer and float)
fn deserialize_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntOrFloat {
        Int(i64),
        Float(f64),
    }

    match IntOrFloat::deserialize(deserializer)? {
        IntOrFloat::Int(i) => Ok(i as f64),
        IntOrFloat::Float(f) => Ok(f),
    }
}

// Default status function
fn default_status() -> String {
    "active".to_string()
}

// Helper to convert CreateBetRequest to Bet for database insertion
impl From<CreateBetRequest> for Bet {
    fn from(req: CreateBetRequest) -> Self {
        let now = Utc::now();

        // Calculate finisher_amount if not provided
        let finisher_amount = req.finisher_amount
            .unwrap_or_else(|| req.total_pot - req.starter_amount);

        Bet {
            id: None, // Will be set by MongoDB
            pledge_id: req.pledge_id.to_string(),
            starter_id: req.starter_id,
            starter_username: req.starter_username,
            starter_selection: req.starter_selection,
            starter_amount: req.starter_amount,
            starter_team: req.starter_team,
            finisher_id: req.finisher_id,
            finisher_username: req.finisher_username,
            finisher_selection: req.finisher_selection,
            finisher_amount,
            finisher_team: req.finisher_team,
            home_team: req.home_team,
            away_team: req.away_team,
            match_time: req.match_time,
            league: req.league,
            sport_type: req.sport_type,
            total_pot: req.total_pot,
            status: req.status,
            winner_id: req.winner_id,
            winner_username: req.winner_username,
            winning_selection: req.winning_selection,
            odds: req.odds,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }
}