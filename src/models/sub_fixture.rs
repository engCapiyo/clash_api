use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubFixtureBet {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub match_id: String,
    pub market_id: String,
    pub starter_id: ObjectId,
    pub starter_name: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub finisher_id: Option<ObjectId>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,
    pub status: BetStatus,
    pub total_pot: f64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub settled_at: Option<DateTime>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BetStatus {
    Open,
    Matched,
    Settled,
    Refunded,
    Cancelled,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubFixtureBetRequest {
    pub match_id: String,
    pub market_id: String,
    pub starter_id: String,
    pub starter_name: String,
    pub selection: String,
    pub amount: f64,
}

#[derive(Debug, Deserialize)]
pub struct FillSubFixtureBetRequest {
    pub match_id: String,
    pub market_id: String,
    pub finisher_id: String,
    pub finisher_name: String,
    pub selection: String,
    pub amount: f64,
}

#[derive(Debug, Serialize)]
pub struct SubFixtureBetResponse {
    pub id: String,
    pub match_id: String,
    pub market_id: String,
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub finisher_id: Option<String>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,
    pub status: String,
    pub total_pot: f64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub settled_at: Option<DateTime>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SubFixtureMarket {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub match_id: String,
    pub market_id: String,
    pub market_type: String,
    pub options: Vec<String>,
    pub line: Option<f64>,
    pub status: String,
    pub lock_at: Option<DateTime>,
    pub pledge_counts: HashMap<String, i32>,
    pub pledge_totals: HashMap<String, i32>,
    pub result: Option<String>,
    pub is_visible: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub settled_at: Option<DateTime>,
}

// Response shape for GET /sub_fixtures/markets/:match_id.
//
// Deliberately separates the Mongo document id ("id", a hex ObjectId)
// from market_id ("marketId", the business key bets reference via
// SubFixtureBet.market_id). The old handler was a stub that returned
// nothing, so this distinction was never surfaced -- the Dart client's
// SubFixtureMarket.fromJson currently collapses both into one `id`
// field and reuses it as market_id when placing a bet. That must be
// fixed client-side to read `marketId` for betting and `id` only as a
// Mongo doc reference, or bets will be created against the wrong key
// the moment market_id and the Mongo _id hex ever diverge.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubFixtureMarketResponse {
    pub id: String,
    pub match_id: String,
    pub market_id: String,
    pub market_type: String,
    pub options: Vec<String>,
    pub line: Option<f64>,
    pub status: String,
    pub lock_at: Option<DateTime>,
    pub pledge_counts: HashMap<String, i32>,
    pub pledge_totals: HashMap<String, i32>,
    pub result: Option<String>,
    pub is_visible: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub settled_at: Option<DateTime>,
}

impl From<SubFixtureMarket> for SubFixtureMarketResponse {
    fn from(m: SubFixtureMarket) -> Self {
        Self {
            id: m.id.map(|oid| oid.to_hex()).unwrap_or_default(),
            match_id: m.match_id,
            market_id: m.market_id,
            market_type: m.market_type,
            options: m.options,
            line: m.line,
            status: m.status,
            lock_at: m.lock_at,
            pledge_counts: m.pledge_counts,
            pledge_totals: m.pledge_totals,
            result: m.result,
            is_visible: m.is_visible,
            created_at: m.created_at,
            updated_at: m.updated_at,
            settled_at: m.settled_at,
        }
    }
}

// Request body for the new HTTP-exposed settlement route
// (settle_sub_fixture_market_handler in sub_fixture_handler.rs).
// winning_team is "home" / "away" / "over" / "under", or omitted/None
// for a draw / no-winner outcome (both sides get refunded).
#[derive(Debug, Deserialize)]
pub struct SettleSubFixtureMarketRequest {
    pub match_id: String,
    pub market_id: String,
    pub winning_team: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct CreateSubFixtureMarketRequest {
    pub match_id: String,
    pub market_type: String,  // "first_goal" | "first_card" | "first_corner"
    pub options: Vec<String>, // e.g. ["home", "away"]
    pub line: Option<f64>,
    pub lock_at: Option<DateTime>,
}

impl From<SubFixtureBet> for SubFixtureBetResponse {
    fn from(bet: SubFixtureBet) -> Self {
        Self {
            id: bet.id.unwrap().to_hex(),
            match_id: bet.match_id,
            market_id: bet.market_id,
            starter_id: bet.starter_id.to_hex(),
            starter_name: bet.starter_name,
            starter_selection: bet.starter_selection,
            starter_amount: bet.starter_amount,
            finisher_id: bet.finisher_id.map(|id| id.to_hex()),
            finisher_name: bet.finisher_name,
            finisher_selection: bet.finisher_selection,
            finisher_amount: bet.finisher_amount,
            status: format!("{:?}", bet.status).to_lowercase(),
            total_pot: bet.total_pot,
            created_at: bet.created_at,
            updated_at: bet.updated_at,
            settled_at: bet.settled_at,
        }
    }
}
