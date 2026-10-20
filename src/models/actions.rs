use bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};

// ============================================================================
// VOTE (Standalone collection)
//
//   - fixture_id : main match id for main-match votes,
//                  market_id (business key) for sub-fixture votes
//   - selection  : "home" | "away" (NO draw). Sub-fixture votes use the
//                  market's own option. Legacy "draw" rows may still exist.
//   - Add a UNIQUE index on (fixture_id, user_id).
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub fixture_id: String,
    pub user_id: String,
    pub user_name: String,
    pub selection: String, // "home" | "away" | market option
    pub is_correct: Option<bool>,
    pub points_awarded: Option<i32>,
    pub voted_at: BsonDateTime,
}

impl Vote {
    pub fn new(fixture_id: String, user_id: String, user_name: String, selection: String) -> Self {
        Self {
            id: None,
            fixture_id,
            user_id,
            user_name,
            selection,
            is_correct: None,
            points_awarded: None,
            voted_at: BsonDateTime::now(),
        }
    }
}

// ============================================================================
// BET (Single collection for all bets — NO channel_id) — money based
// Selections are "home" | "away" only (no draw).
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bet {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub fixture_id: String,

    // === STARTER ===
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home" | "away"
    pub starter_amount: f64,

    // === FINISHER ===
    pub finisher_id: Option<String>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,

    // === VOTE REFERENCE ===
    pub vote_id: Option<String>,

    // === STATUS ===
    pub status: String, // "open", "matched", "settled", "refunded"

    // === RESULT ===
    pub winner_id: Option<String>,
    pub starter_result: Option<String>, // "won", "lost", "draw" (match drawn => refund), "unmatched"
    pub finisher_result: Option<String>,

    // === TIMESTAMPS ===
    pub created_at: BsonDateTime,
    pub matched_at: Option<BsonDateTime>,
    pub settled_at: Option<BsonDateTime>,
}

impl Bet {
    pub fn new_open(
        fixture_id: String,
        starter_id: String,
        starter_name: String,
        starter_selection: String,
        amount: f64,
        vote_id: Option<String>,
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
            vote_id,
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
        self.status == "settled" || self.status == "refunded"
    }

    pub fn total_pot(&self) -> f64 {
        self.starter_amount + self.finisher_amount.unwrap_or(0.0)
    }
}

// ============================================================================
// POKE (Single collection — POINTS based, main fixture OR sub-fixture market)
//
//   - fixture_id : always the main match id
//   - market_id  : None => poke is on the main match ("home" | "away")
//                  Some => poke is on that sub-fixture market
//                          (business key, e.g. "{match_id}_{market_type}")
//   - stake      : points each side puts in (finisher matches exactly)
//   - pot        : stake * 2 once matched
//   - No draw selection. If the match ends drawn, or a market has no
//     winner, both stakes are refunded (result = "refunded").
// ============================================================================

pub const MIN_POKE_POINTS: i32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poke {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<bson::oid::ObjectId>,
    pub fixture_id: String, // main match id
    #[serde(default)]
    pub market_id: Option<String>, // None => main match, Some => sub-fixture market_id

    // === STARTER ===
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home" | "away" | market option

    // === STAKE (points, equal on both sides) ===
    pub stake: i32,

    // === FINISHER ===
    pub finisher_id: Option<String>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,

    // === STATUS ===
    pub status: String, // "open" | "matched" | "settled" | "refunded"

    // === RESULT ===
    pub winner_id: Option<String>,
    pub starter_result: Option<String>, // "won" | "lost" | "refunded" | "unmatched"
    pub finisher_result: Option<String>,

    // === TIMESTAMPS ===
    pub created_at: BsonDateTime,
    pub matched_at: Option<BsonDateTime>,
    pub settled_at: Option<BsonDateTime>,
}

impl Poke {
    pub fn new_open(
        fixture_id: String,
        market_id: Option<String>,
        starter_id: String,
        starter_name: String,
        starter_selection: String,
        stake: i32,
    ) -> Self {
        Self {
            id: None,
            fixture_id,
            market_id,
            starter_id,
            starter_name,
            starter_selection,
            stake,
            finisher_id: None,
            finisher_name: None,
            finisher_selection: None,
            status: "open".to_string(),
            winner_id: None,
            starter_result: None,
            finisher_result: None,
            created_at: BsonDateTime::now(),
            matched_at: None,
            settled_at: None,
        }
    }

    /// Key used in the votes collection:
    /// market_id for sub-fixtures, fixture_id for the main match.
    pub fn vote_key(&self) -> String {
        self.market_id
            .clone()
            .unwrap_or_else(|| self.fixture_id.clone())
    }

    pub fn is_open(&self) -> bool {
        self.status == "open"
    }

    pub fn is_matched(&self) -> bool {
        self.status == "matched"
    }

    pub fn is_settled(&self) -> bool {
        self.status == "settled" || self.status == "refunded"
    }

    pub fn total_pot(&self) -> i32 {
        if self.finisher_id.is_some() {
            self.stake * 2
        } else {
            self.stake
        }
    }
}

// ============================================================================
// REQUESTS — NO channel_id on universal actions
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CastVoteRequest {
    pub fixture_id: String,
    pub user_id: String,
    pub username: String,
    pub selection: String, // "home" | "away"
}

#[derive(Debug, Deserialize)]
pub struct CreateBetRequest {
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home" | "away"
    pub amount: f64,
    pub fixture_id: String,
    pub vote_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FillBetRequest {
    pub bet_id: String,
    pub finisher_id: String,
    pub finisher_name: String,
    pub finisher_selection: String, // "home" | "away"
    pub amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct SettleBetRequest {
    pub fixture_id: String,
    pub result: String, // "home" | "away" | "draw" (draw => refund)
}

#[derive(Debug, Deserialize)]
pub struct RollbackVoteRequest {
    pub fixture_id: String, // main fixture id OR market_id
    pub user_id: String,
}

// ---------------------------- POKE REQUESTS ---------------------------------

#[derive(Debug, Deserialize)]
pub struct CreatePokeRequest {
    pub fixture_id: String,
    #[serde(default)]
    pub market_id: Option<String>, // None => main match
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String, // "home" | "away" | market option
    pub stake: i32,                // points
}

#[derive(Debug, Deserialize)]
pub struct FillPokeRequest {
    pub poke_id: String,
    pub finisher_id: String,
    pub finisher_name: String,
    pub finisher_selection: String,
    // NO amount: the finisher always matches the starter's stake exactly.
}

#[derive(Debug, Deserialize)]
pub struct SettlePokeRequest {
    pub fixture_id: String,
    #[serde(default)]
    pub market_id: Option<String>, // None => settle main-match pokes
    #[serde(default)]
    pub result: Option<String>, // winning selection; None (or "draw" on main) => refund all
}

#[derive(Debug, Deserialize)]
pub struct PokeListQuery {
    #[serde(default)]
    pub market_id: Option<String>,
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
    pub vote_id: Option<String>,
    pub status: String,
    pub created_at: BsonDateTime,
    pub matched_at: Option<BsonDateTime>,
}