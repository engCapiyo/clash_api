use chrono::{DateTime, Utc};
use mongodb::bson::{doc, oid::ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchiveActivity {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub home_team: String,
    pub away_team: String,
    pub activity_type: ActivityType,
    pub selection: Option<String>, // Only for votes: "home_team", "draw", "away_team"
    pub is_liked: Option<bool>,    // Only for likes: true/false
    pub comment: Option<String>,   // Only for comments: comment text
    pub timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ActivityType {
    Vote,
    Like,
    Comment,
}

// Implement conversion to string for ActivityType
impl ActivityType {
    pub fn as_str(&self) -> &str {
        match self {
            ActivityType::Vote => "vote",
            ActivityType::Like => "like",
            ActivityType::Comment => "comment",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchiveActivityRequest {
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub home_team: String,
    pub away_team: String,
    pub activity_type: String, // "vote", "like", "comment"
    pub selection: Option<String>,
    pub is_liked: Option<bool>,
    pub comment: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchiveActivityResponse {
    pub success: bool,
    pub message: String,
    pub activity_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserArchiveStats {
    pub user_id: String,
    pub total_votes: i32,
    pub total_likes: i32,
    pub total_comments: i32,
    pub recent_activities: Vec<ArchiveActivity>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchiveQueryParams {
    pub user_id: Option<String>,
    pub fixture_id: Option<String>,
    pub activity_type: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

// NEW: Query parameters for getting all archives
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GetAllArchiveQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub username: Option<String>,
    pub user_id: Option<String>,
    pub activity_type: Option<String>,
    pub sort_by: Option<String>, // "newest" or "oldest"
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

// NEW: Response structure for paginated archives
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PaginatedArchiveResponse {
    pub success: bool,
    pub data: Vec<ArchiveActivity>,
    pub pagination: PaginationInfo,
    pub stats: ArchiveStats,
    pub filters_applied: AppliedFilters,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PaginationInfo {
    pub page: i64,
    pub limit: i64,
    pub total_items: i64,
    pub total_pages: i64,
    pub has_next: bool,
    pub has_prev: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchiveStats {
    pub unique_users: i32,
    pub returned_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_breakdown: Option<Vec<ActivityBreakdown>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActivityBreakdown {
    #[serde(rename = "_id")]
    pub activity_type: String,
    pub count: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppliedFilters {
    pub username: Option<String>,
    pub user_id: Option<String>,
    pub activity_type: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub sort_by: String,
}
