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
