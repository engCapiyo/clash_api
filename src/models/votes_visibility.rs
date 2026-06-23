// models/votes_visibility.rs

use chrono::{DateTime, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VotesVisibility {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub key: String,
    pub value: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "default_description")]
    pub description: String,
    pub updated_at: DateTime<Utc>,
}

fn default_description() -> String {
    "Show/hide votes button globally".to_string()
}

impl VotesVisibility {
    pub fn new(key: String, value: bool) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            key,
            value,
            created_at: now,
            description: "Show/hide votes button globally".to_string(),
            updated_at: now,
        }
    }
}
