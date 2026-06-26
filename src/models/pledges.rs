use chrono::{DateTime, Utc};
use mongodb::bson;
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pledge {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    pub username: String,
    pub phone: String,
    pub selection: String, // "home_team", "away_team", or "draw"
    pub amount: f64,
    pub time: DateTime<Utc>,
    pub fan: String,
    pub home_team: String,
    pub away_team: String,
    pub starter_id: String,
    pub fixture_id: Option<String>, // ✅ ADDED: Links pledge to a specific fixture

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePledge {
    pub username: String,
    pub phone: String,
    pub selection: String,
    pub amount: f64,
    pub fan: String,
    pub home_team: String,
    pub away_team: String,
    pub starter_id: String,
    pub fixture_id: Option<String>, // ✅ ADDED: Links pledge to a specific fixture
}

#[derive(Debug, Deserialize)]
pub struct PledgeQuery {
    pub username: Option<String>,
    pub phone: Option<String>,
    pub home_team: Option<String>,
    pub away_team: Option<String>,
    pub starter_id: Option<String>,
    pub fixture_id: Option<String>, // ✅ ADDED: Filter pledges by fixture
}
