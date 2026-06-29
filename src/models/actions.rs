// File: src/models/actions.rs

use bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};

// ============================================================================
// VOTER (Global - No channel_id)
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voter {
    pub user_id: String,
    pub user_name: String,
    pub selection: String, // "home", "away", "draw"
    pub is_correct: Option<bool>,
    pub points_awarded: Option<i32>,
    pub voted_at: BsonDateTime,
}

// ============================================================================
// BET (Single collection for all bets)
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bet {
    // === IDENTIFIERS ===
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub fixture_id: String,

    // === STARTER (who created the bet) ===
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home", "away", "draw"
    pub starter_amount: f64,

    // === FINISHER (who accepted the bet) ===
    pub finisher_id: Option<String>, // null = open
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,

    // === VOTE REFERENCE ===
    pub vote_id: Option<String>, // ✅ NEW: Reference to vote

    // === CHANNEL (where the bet is visible) ===
    pub channel_id: String, // Channel-specific visibility

    // === STATUS ===
    pub status: String, // "open", "matched", "settled"

    // === RESULT ===
    pub winner_id: Option<String>,
    pub starter_result: Option<String>, // "won", "lost", "draw"
    pub finisher_result: Option<String>,

    // === TIMESTAMPS ===
    pub created_at: BsonDateTime,
    pub matched_at: Option<BsonDateTime>,
    pub settled_at: Option<BsonDateTime>,
}

// ============================================================================
// REQUESTS
// ============================================================================

// Cast Vote Request (Global)
#[derive(Debug, Deserialize)]
pub struct CastVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
    pub username: String,
    pub selection: String, // "home", "away", "draw"
}

// ✅ UPDATED: Create Bet Request (with vote_id)
#[derive(Debug, Deserialize)]
pub struct CreateBetRequest {
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home", "away", "draw"
    pub amount: f64,
    pub fixture_id: String,
    pub channel_id: String,
    pub vote_id: String, // ✅ NEW: Required vote reference
}

// Fill Bet Request (Finisher)
#[derive(Debug, Deserialize)]
pub struct FillBetRequest {
    pub bet_id: String,
    pub finisher_id: String,
    pub finisher_name: String,
    pub finisher_selection: String, // "home", "away", "draw"
    pub amount: f64,
    pub channel_id: String,
}

// Settle Bet Request (Live Poller)
#[derive(Debug, Deserialize)]
pub struct SettleBetRequest {
    pub fixture_id: String,
    pub result: String, // "home", "away", "draw"
}

// ============================================================================
// RESPONSES
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub fixture_id: String,
    pub selection: String,
    pub voted_at: BsonDateTime,
}

#[derive(Debug, Serialize)]
pub struct BetResponse {
    pub bet_id: String,
    pub fixture_id: String,
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub finisher_id: Option<String>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,
    pub vote_id: Option<String>, // ✅ NEW
    pub status: String,
    pub created_at: BsonDateTime,
    pub matched_at: Option<BsonDateTime>,
}

// ============================================================================
// DEFAULTS
// ============================================================================

impl Bet {
    pub fn new_open(
        fixture_id: String,
        starter_id: String,
        starter_name: String,
        starter_selection: String,
        amount: f64,
        channel_id: String,
        vote_id: String, // ✅ NEW
    ) -> Self {
        let now = BsonDateTime::now();
        Self {
            id: None,
            fixture_id,
            starter_id,
            starter_name,
            starter_selection,
            starter_amount: amount,
            finisher_id: None,
            finisher_name: None,
            finisher_selection: None,
            finisher_amount: None,
            vote_id: Some(vote_id), // ✅ NEW
            channel_id,
            status: "open".to_string(),
            winner_id: None,
            starter_result: None,
            finisher_result: None,
            created_at: now,
            matched_at: None,
            settled_at: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.status == "open"
    }

    pub fn is_matched(&self) -> bool {
        self.status == "matched"
    }

    pub fn is_settled(&self) -> bool {
        self.status == "settled"
    }

    pub fn total_pot(&self) -> f64 {
        self.starter_amount + self.finisher_amount.unwrap_or(0.0)
    }
}
