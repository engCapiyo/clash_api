// posterId
// postername
// postId
// poster_club
// comment
// time

use bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use crate::models::game::Game;  // Import Game model

// ========== COMMENT MODELS ==========

// Main Comment model - matches your MongoDB documents EXACTLY
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostComments {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "poster_id")]
    pub poster_id: String,

    #[serde(rename = "postername")]
    pub postername: String,

    #[serde(rename = "post_id")]
    pub post_id: String,

    #[serde(rename = "comment")]
    pub comment: String,

    #[serde(rename = "time")]
    pub time: String,

    #[serde(rename = "created_at", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<BsonDateTime>,
}

// For creating new comments
#[derive(Debug, Deserialize,Serialize)]
pub struct CreateComment {
    pub poster_id: String,
    pub postername: String,
    pub post_id: String,
    pub comment: String,
    pub date: String,
    pub time: String,
}

// For updating comments (if needed)
#[derive(Debug, Deserialize)]
pub struct UpdateComment {
    pub comment: String,
    pub time: String,
}

// ========== COMMENT RESPONSE ==========

// Response for a single comment
#[derive(Debug, Serialize)]
pub struct CommentResponse {
    #[serde(rename = "_id")]
    pub id: String,
    pub poster_id: String,
    pub postername: String,
    pub post_id: String,
    pub comment: String,
    pub time: String,
    pub created_at: String,
    pub formatted_time: String,
}

impl From<PostComments> for CommentResponse {
    fn from(comment: PostComments) -> Self {
        use chrono::{DateTime, Utc};

        let created_at = comment.created_at.unwrap_or_else(|| BsonDateTime::from_chrono(Utc::now()));
        let chrono_dt: DateTime<Utc> = created_at.into();

        // Format time for display
        let now = Utc::now();
        let duration = now.signed_duration_since(chrono_dt);

        let formatted_time = if duration.num_minutes() < 1 {
            "Just now".to_string()
        } else if duration.num_hours() < 1 {
            format!("{}m ago", duration.num_minutes())
        } else if duration.num_days() < 1 {
            format!("{}h ago", duration.num_hours())
        } else if duration.num_days() == 1 {
            "Yesterday".to_string()
        } else {
            chrono_dt.format("%b %d").to_string()
        };

        Self {
            id: comment.id.map_or("".to_string(), |id| id.to_hex()),
            poster_id: comment.poster_id,
            postername: comment.postername,
            post_id: comment.post_id,
            comment: comment.comment,
            time: comment.time,
            created_at: chrono_dt.to_rfc3339(),
            formatted_time,
        }
    }
}

// ========== QUERY PARAMETERS ==========

#[derive(Debug, Deserialize)]
pub struct CommentQuery {
    pub poster_id: Option<String>,
    pub post_id: Option<String>,
    pub limit: Option<i64>,
    pub page: Option<u64>,
    pub sort_by: Option<String>, // "newest", "oldest"
}

// ========== API RESPONSE WRAPPERS ==========

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
    pub message: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data,
            message: None,
        }
    }

    pub fn with_message(data: T, message: String) -> Self {
        Self {
            success: true,
            data,
            message: Some(message),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
    pub message: String,
    pub timestamp: String,
}

impl ErrorResponse {
    pub fn new(error: &str, message: &str) -> Self {
        use chrono::Utc;
        Self {
            success: false,
            error: error.to_string(),
            message: message.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

// ========== PAGINATED RESPONSES ==========

#[derive(Debug, Serialize)]
pub struct PaginatedComments {
    pub comments: Vec<CommentResponse>,
    pub total: i64,
    pub page: u64,
    pub limit: i64,
    pub total_pages: i64,
}

// For games-related responses (if needed)
#[derive(Debug, Serialize)]
pub struct PaginatedGames {
    pub games: Vec<Game>,
    pub total: i64,
    pub page: u64,
    pub limit: i64,
    pub total_pages: i64,
}

// For live games response
#[derive(Debug, Serialize)]
pub struct LiveGamesResponse {
    pub live_games: Vec<Game>,
    pub count: usize,
    pub last_updated: String,
}

// ========== COMMENT STATISTICS ==========

#[derive(Debug, Serialize)]
pub struct CommentStats {
    #[serde(rename = "postId")]
    pub post_id: String,

    #[serde(rename = "totalComments")]
    pub total_comments: i64,

    #[serde(rename = "uniqueCommenters")]
    pub unique_commenters: i64,

    #[serde(rename = "lastCommentAt")]
    pub last_comment_at: Option<String>,
}

// ========== BULK OPERATIONS ==========

#[derive(Debug, Deserialize)]
pub struct BulkCommentRequest {
    pub comments: Vec<CreateComment>,
}

#[derive(Debug, Serialize)]
pub struct BulkCommentResponse {
    pub success: bool,
    pub inserted_count: usize,
    pub failed_count: usize,
    pub failed_comments: Vec<FailedComment>,
}

#[derive(Debug, Serialize)]
pub struct FailedComment {
    pub index: usize,
    pub error: String,
    pub comment: CreateComment,
}

// ========== DEFAULT IMPLEMENTATIONS ==========

impl Default for PostComments {
    fn default() -> Self {
        Self {
            id: None,
            poster_id: String::new(),
            postername: String::new(),
            post_id: String::new(),
            comment: String::new(),
            time: String::new(),
            created_at: Some(BsonDateTime::from_chrono(chrono::Utc::now())),
        }
    }
}

// ========== HELPER FUNCTIONS ==========

pub fn validate_comment(comment: &str) -> Result<(), String> {
    if comment.trim().is_empty() {
        return Err("Comment cannot be empty".to_string());
    }
    if comment.len() > 1000 {
        return Err("Comment must be less than 1000 characters".to_string());
    }
    Ok(())
}

pub fn validate_poster_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Poster name cannot be empty".to_string());
    }
    if name.len() > 50 {
        return Err("Poster name must be less than 50 characters".to_string());
    }
    Ok(())
}
