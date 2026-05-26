use bson::{oid::ObjectId, DateTime as BsonDateTime};
use chrono::Utc;
use serde::{Deserialize, Serialize};

// ========== SUB-FIXTURE (PROP BET) MODEL ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubFixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub sub_fixture_id: String,
    pub parent_fixture_id: String,
    pub fixture_type: String,
    pub question: String,
    pub option_a: String,
    pub option_b: String,
    pub option_c: Option<String>,
    pub odds_a: f64,
    pub odds_b: f64,
    pub odds_c: Option<f64>,
    pub is_active: bool,
    pub display_order: i32,
    pub icon: String,
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

// ========== SUB-FIXTURE VOTE MODEL ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubFixtureVote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub voter_id: String,
    pub username: String,
    pub sub_fixture_id: String,
    pub parent_fixture_id: String,
    pub selection: String,
    pub voted_at: BsonDateTime,
    pub created_at: BsonDateTime,
}

impl SubFixtureVote {
    /// Create a new SubFixtureVote with current timestamp
    pub fn new(
        voter_id: &str,
        username: &str,
        sub_fixture_id: &str,
        parent_fixture_id: &str,
        selection: &str,
    ) -> Self {
        let now = BsonDateTime::from_chrono(Utc::now());
        Self {
            id: None,
            voter_id: voter_id.to_string(),
            username: username.to_string(),
            sub_fixture_id: sub_fixture_id.to_string(),
            parent_fixture_id: parent_fixture_id.to_string(),
            selection: selection.to_string(),
            voted_at: now,
            created_at: now,
        }
    }
}

// ========== REQUEST MODELS ==========
#[derive(Debug, Deserialize)]
pub struct CreateSubFixtureVoteRequest {
    pub voter_id: String,
    pub username: String,
    pub sub_fixture_id: String,
    pub parent_fixture_id: String,
    pub selection: String,
    // Optional fields for auto-creation
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub option_a: Option<String>,
    #[serde(default)]
    pub option_b: Option<String>,
    #[serde(default)]
    pub option_c: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub fixture_type: Option<String>, // ✅ FIX: Added fixture_type field
}

#[derive(Debug, Deserialize)]
pub struct CreateSubFixtureRequest {
    pub parent_fixture_id: String,
    pub fixture_type: String,
    pub question: String,
    pub option_a: String,
    pub option_b: String,
    pub option_c: Option<String>,
    pub odds_a: f64,
    pub odds_b: f64,
    pub odds_c: Option<f64>,
    pub display_order: i32,
    pub icon: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSubFixtureRequest {
    pub question: Option<String>,
    pub option_a: Option<String>,
    pub option_b: Option<String>,
    pub option_c: Option<String>,
    pub odds_a: Option<f64>,
    pub odds_b: Option<f64>,
    pub odds_c: Option<f64>,
    pub is_active: Option<bool>,
    pub display_order: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct SubFixtureQuery {
    pub parent_fixture_id: Option<String>,
    pub fixture_type: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct VotersQuery {
    pub selection: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct BulkStatsRequest {
    pub sub_fixture_ids: Vec<String>,
}

// ========== RESPONSE MODELS ==========
#[derive(Debug, Serialize)]
pub struct SubFixtureVoteResponse {
    pub success: bool,
    pub message: String,
    pub vote_id: Option<String>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SubFixtureStats {
    pub sub_fixture_id: String,
    pub question: String,
    pub total_votes: i64,
    pub option_a_votes: i64,
    pub option_b_votes: i64,
    pub option_c_votes: Option<i64>,
    pub option_a_percentage: f64,
    pub option_b_percentage: f64,
    pub option_c_percentage: Option<f64>,
    pub user_vote: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VoterInfo {
    pub voter_id: String,
    pub username: String,
    pub selection: String,
    pub voted_at: BsonDateTime,
}
