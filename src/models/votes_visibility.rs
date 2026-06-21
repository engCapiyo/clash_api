// models/votes_visibility.rs

use chrono::{DateTime, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VotesVisibility {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub key: String, // e.g., "votes_button_show"
    pub value: bool,
    pub updated_at: DateTime<Utc>,
}
