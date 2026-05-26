// src/models/comrade.rs - simplified

use chrono::{DateTime, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comrade {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,    // The user who added the comrade
    pub comrade_id: String, // The comrade being added
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
    pub status: String, // "active" or "pending"
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AddComradeRequest {
    pub user_id: String,
    pub comrade_id: String,
    pub username: String,
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
}

#[derive(Debug, Serialize)]
pub struct ComradeResponse {
    pub comrade_id: String,
    pub comrade_username: String,
    pub comrade_nickname: String,
    pub comrade_club: String,
    pub comrade_country: String,
    pub added_at: DateTime<Utc>,
}
