// File: src/models/comment_models.rs

use bson::{oid::ObjectId, DateTime as BsonDateTime};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

// ============================================================================
// COMMENT MODEL - Full featured with reply support
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Comment {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "postId")]
    #[validate(length(min = 1, message = "Post ID is required"))]
    pub post_id: String,

    #[serde(rename = "userId")]
    #[validate(length(min = 1, message = "User ID is required"))]
    pub user_id: String,

    #[serde(rename = "userName")]
    #[validate(length(min = 1, message = "User name is required"))]
    pub user_name: String,

    #[serde(rename = "comment")]
    #[validate(length(min = 1, max = 2000, message = "Comment must be between 1 and 2000 characters"))]
    pub comment: String,

    #[serde(rename = "likesCount")]
    pub likes_count: i32,

    #[serde(rename = "likedBy")]
    pub liked_by: Vec<String>,

    #[serde(rename = "parentCommentId", skip_serializing_if = "Option::is_none")]
    pub parent_comment_id: Option<String>,

    #[serde(rename = "replyCount")]
    pub reply_count: i32,

    #[serde(rename = "replies", skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<Comment>>,

    #[serde(rename = "createdAt")]
    pub created_at: BsonDateTime,

    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<BsonDateTime>,

    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<BsonDateTime>,

    #[serde(rename = "timestamp")]
    pub timestamp: i64,
}

impl Comment {
    pub fn new(
        post_id: String,
        user_id: String,
        user_name: String,
        comment: String,
        parent_comment_id: Option<String>,
    ) -> Self {
        let now = BsonDateTime::now();
        let timestamp = Utc::now().timestamp();

        Self {
            id: Some(ObjectId::new()),
            post_id,
            user_id,
            user_name,
            comment,
            likes_count: 0,
            liked_by: Vec::new(),
            parent_comment_id,
            reply_count: 0,
            replies: None,
            created_at: now,
            updated_at: None,
            last_modified: Some(now),
            timestamp,
        }
    }

    pub fn is_reply(&self) -> bool {
        self.parent_comment_id.is_some()
    }

    pub fn has_replies(&self) -> bool {
        self.reply_count > 0 && self.replies.is_some()
    }

    pub fn increment_likes(&mut self) {
        self.likes_count += 1;
        self.updated_at = Some(BsonDateTime::now());
        self.last_modified = Some(BsonDateTime::now());
    }

    pub fn decrement_likes(&mut self) {
        if self.likes_count > 0 {
            self.likes_count -= 1;
            self.updated_at = Some(BsonDateTime::now());
            self.last_modified = Some(BsonDateTime::now());
        }
    }

    pub fn add_like(&mut self, user_id: String) {
        if !self.liked_by.contains(&user_id) {
            self.liked_by.push(user_id);
            self.likes_count = self.liked_by.len() as i32;
            self.updated_at = Some(BsonDateTime::now());
            self.last_modified = Some(BsonDateTime::now());
        }
    }

    pub fn remove_like(&mut self, user_id: &str) {
        self.liked_by.retain(|id| id != user_id);
        self.likes_count = self.liked_by.len() as i32;
        self.updated_at = Some(BsonDateTime::now());
        self.last_modified = Some(BsonDateTime::now());
    }

    pub fn add_reply(&mut self, reply: Comment) {
        self.reply_count += 1;
        if let Some(ref mut replies) = self.replies {
            replies.push(reply);
        } else {
            self.replies = Some(vec![reply]);
        }
        self.updated_at = Some(BsonDateTime::now());
        self.last_modified = Some(BsonDateTime::now());
    }

    pub fn remove_reply(&mut self, reply_id: &str) -> bool {
        if let Some(ref mut replies) = self.replies {
            let original_len = replies.len();
            replies.retain(|r| {
                r.id.as_ref()
                    .map(|id| id.to_hex() != reply_id)
                    .unwrap_or(true)
            });
            if replies.len() < original_len {
                self.reply_count -= 1;
                self.updated_at = Some(BsonDateTime::now());
                self.last_modified = Some(BsonDateTime::now());
                return true;
            }
        }
        false
    }

    pub fn has_user_liked(&self, user_id: &str) -> bool {
        self.liked_by.contains(&user_id.to_string())
    }

    pub fn get_like_count(&self) -> i32 {
        self.likes_count
    }

    pub fn get_reply_count(&self) -> i32 {
        self.reply_count
    }
}

// ============================================================================
// CREATE COMMENT REQUEST
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateCommentRequest {
    #[serde(rename = "user_id")]
    #[validate(length(min = 1, message = "User ID is required"))]
    pub user_id: String,

    #[serde(rename = "user_name")]
    #[validate(length(min = 1, message = "User name is required"))]
    pub user_name: String,

    #[serde(rename = "comment")]
    #[validate(length(min = 1, max = 2000, message = "Comment must be between 1 and 2000 characters"))]
    pub comment: String,

    #[serde(rename = "parent_comment_id", skip_serializing_if = "Option::is_none")]
    pub parent_comment_id: Option<String>,
}

// ============================================================================
// UPDATE COMMENT REQUEST
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateCommentRequest {
    #[serde(rename = "user_id")]
    #[validate(length(min = 1, message = "User ID is required"))]
    pub user_id: String,

    #[serde(rename = "comment")]
    #[validate(length(min = 1, max = 2000, message = "Comment must be between 1 and 2000 characters"))]
    pub comment: String,
}

// ============================================================================
// LIKE COMMENT REQUEST
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct LikeCommentRequest {
    #[serde(rename = "user_id")]
    pub user_id: String,

    // NOTE: `default = "..."` must name a function that returns the field
    // type. A literal `""` is not a valid path, so this failed to compile.
    // `String` already implements `Default` (empty string), so a bare
    // `#[serde(default)]` is enough.
    #[serde(rename = "user_name", default)]
    pub user_name: String,
}

// ============================================================================
// GET COMMENTS QUERY
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GetCommentsQuery {
    #[serde(default = "default_sort")]
    pub sort: String,

    #[serde(default = "default_limit")]
    pub limit: i64,

    #[serde(default = "default_page")]
    pub page: i64,

    #[serde(default = "default_false")]
    pub exclude_replies: bool,

    #[serde(default)]
    pub user_id: Option<String>,

    #[serde(default)]
    pub post_id: Option<String>,
}

fn default_sort() -> String { "newest".to_string() }
fn default_limit() -> i64 { 20 }
fn default_page() -> i64 { 1 }
fn default_false() -> bool { false }

// ============================================================================
// COMMENT RESPONSE
// ============================================================================

#[derive(Debug, Serialize, Clone)]
pub struct CommentResponse {
    #[serde(rename = "id")]
    pub id: String,

    #[serde(rename = "postId")]
    pub post_id: String,

    #[serde(rename = "userId")]
    pub user_id: String,

    #[serde(rename = "userName")]
    pub user_name: String,

    #[serde(rename = "comment")]
    pub comment: String,

    #[serde(rename = "likesCount")]
    pub likes_count: i32,

    #[serde(rename = "likedBy")]
    pub liked_by: Vec<String>,

    #[serde(rename = "parentCommentId")]
    pub parent_comment_id: Option<String>,

    #[serde(rename = "replyCount")]
    pub reply_count: i32,

    #[serde(rename = "replies", skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<CommentResponse>>,

    #[serde(rename = "createdAt")]
    pub created_at: String,

    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,

    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,

    #[serde(rename = "timestamp")]
    pub timestamp: i64,

    #[serde(rename = "formattedTime")]
    pub formatted_time: String,
}

impl From<Comment> for CommentResponse {
    fn from(comment: Comment) -> Self {
        let created_at: DateTime<Utc> = comment.created_at.into();
        let formatted_time = format_time_ago(created_at);

        let updated_at = comment.updated_at.map(|dt| {
            let dt: DateTime<Utc> = dt.into();
            dt.to_rfc3339()
        });

        let last_modified = comment.last_modified.map(|dt| {
            let dt: DateTime<Utc> = dt.into();
            dt.to_rfc3339()
        });

        let replies = comment.replies.map(|replies| {
            replies.into_iter().map(CommentResponse::from).collect()
        });

        CommentResponse {
            id: comment.id.map_or("".to_string(), |id| id.to_hex()),
            post_id: comment.post_id,
            user_id: comment.user_id,
            user_name: comment.user_name,
            comment: comment.comment,
            likes_count: comment.likes_count,
            liked_by: comment.liked_by,
            parent_comment_id: comment.parent_comment_id,
            reply_count: comment.reply_count,
            replies,
            created_at: created_at.to_rfc3339(),
            updated_at,
            last_modified,
            timestamp: comment.timestamp,
            formatted_time,
        }
    }
}

fn format_time_ago(date: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(date);

    if duration.num_minutes() < 1 {
        "Just now".to_string()
    } else if duration.num_hours() < 1 {
        format!("{}m", duration.num_minutes())
    } else if duration.num_days() < 1 {
        format!("{}h", duration.num_hours())
    } else if duration.num_days() < 7 {
        format!("{}d", duration.num_days())
    } else if duration.num_days() < 30 {
        format!("{} days ago", duration.num_days())
    } else if duration.num_days() < 365 {
        format!("{}mo", duration.num_days() / 30)
    } else {
        // Requires `chrono::Datelike` to be in scope for .month()/.year().
        format!("{}/{}", date.month(), date.year())
    }
}

// ============================================================================
// PAGINATED COMMENTS RESPONSE
// ============================================================================

#[derive(Debug, Serialize)]
pub struct PaginatedCommentsResponse {
    pub comments: Vec<CommentResponse>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    #[serde(rename = "totalPages")]
    pub total_pages: i64,
}

// ============================================================================
// COMMENT COUNT RESPONSE
// ============================================================================

#[derive(Debug, Serialize)]
pub struct CommentCountResponse {
    #[serde(rename = "postId")]
    pub post_id: String,
    pub count: i32,
}

// ============================================================================
// API RESPONSE WRAPPER
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            message: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            message: Some(message),
        }
    }

    pub fn error_with_data(message: String, data: T) -> Self {
        Self {
            success: false,
            data: Some(data),
            message: Some(message),
        }
    }
}

// ============================================================================
// ERROR RESPONSE
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
    pub message: String,
    pub timestamp: String,
}

impl ErrorResponse {
    pub fn new(error: &str, message: &str) -> Self {
        Self {
            success: false,
            error: error.to_string(),
            message: message.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}