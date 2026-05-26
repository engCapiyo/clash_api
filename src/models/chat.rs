use bson::{oid::ObjectId, DateTime as BsonDateTime};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

// ========== CHAT MESSAGE MODEL ==========

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ChatMessage {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "postId")]
    #[validate(length(min = 1, message = "Post ID is required"))]
    pub post_id: String,

    #[serde(rename = "senderId")]
    #[validate(length(min = 1, message = "Sender ID is required"))]
    pub sender_id: String,

    #[serde(rename = "receiverId")]
    #[validate(length(min = 1, message = "Receiver ID is required"))]
    pub receiver_id: String,

    #[serde(rename = "senderName")]
    #[validate(length(min = 1, message = "Sender name is required"))]
    pub sender_name: String,

    #[serde(rename = "receiverName")]
    #[validate(length(min = 1, message = "Receiver name is required"))]
    pub receiver_name: String,  // ADDED THIS FIELD

    #[serde(rename = "message")]
    #[validate(length(min = 1, max = 1000, message = "Message must be between 1 and 1000 characters"))]
    pub message: String,

    #[serde(rename = "seen")]
    pub seen: bool,

    #[serde(rename = "createdAt")]
    pub created_at: BsonDateTime,

    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<BsonDateTime>,
}

// ========== CREATE MESSAGE REQUEST ==========

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatMessage {
    #[serde(rename = "sender_id")]
    #[validate(length(min = 1, message = "Sender ID is required"))]
    pub sender_id: String,

    #[serde(rename = "receiver_id")]
    #[validate(length(min = 1, message = "Receiver ID is required"))]
    pub receiver_id: String,

    #[serde(rename = "sender_name")]
    #[validate(length(min = 1, message = "Sender name is required"))]
    pub sender_name: String,

    #[serde(rename = "receiver_name")]  // ADDED THIS FIELD
    #[validate(length(min = 1, message = "Receiver name is required"))]
    pub receiver_name: String,

    #[serde(rename = "message")]
    #[validate(length(min = 1, max = 1000, message = "Message must be between 1 and 1000 characters"))]
    pub message: String,
}

// ========== UPDATE MESSAGE REQUEST ==========

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChatMessage {
    #[serde(rename = "message")]
    #[validate(length(min = 1, max = 1000, message = "Message must be between 1 and 1000 characters"))]
    pub message: String,
}

// ========== MARK AS SEEN REQUEST ==========

#[derive(Debug, Deserialize)]
pub struct MarkAsSeenRequest {
    #[serde(rename = "post_id")]
    pub post_id: String,

    #[serde(rename = "user_id")]
    pub user_id: String,
}

// ========== CHAT MESSAGE RESPONSE ==========

#[derive(Debug, Serialize, Clone)]
pub struct ChatMessageResponse {
    #[serde(rename = "id")]
    pub id: String,

    #[serde(rename = "postId")]
    pub post_id: String,

    #[serde(rename = "senderId")]
    pub sender_id: String,

    #[serde(rename = "receiverId")]
    pub receiver_id: String,

    #[serde(rename = "senderName")]
    pub sender_name: String,

    #[serde(rename = "receiverName")]  // ADDED THIS FIELD
    pub receiver_name: String,

    #[serde(rename = "message")]
    pub message: String,

    #[serde(rename = "seen")]
    pub seen: bool,

    #[serde(rename = "createdAt")]
    pub created_at: String,

    #[serde(rename = "formattedTime")]
    pub formatted_time: String,
}

// ========== PAGINATION QUERY ==========

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

// ========== API RESPONSE WRAPPER ==========

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
}

// ========== PAGINATED RESPONSE ==========

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub messages: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    #[serde(rename = "totalPages")]
    pub total_pages: i64,
}

// ========== CONVERSION IMPLEMENTATIONS ==========

impl From<ChatMessage> for ChatMessageResponse {
    fn from(msg: ChatMessage) -> Self {
        let created_at = msg.created_at;

        // Convert BsonDateTime to chrono DateTime for formatting
        let chrono_dt: DateTime<Utc> = created_at.into();

        // Format time for display (matches Flutter's expectations)
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
            id: msg.id.map_or("".to_string(), |id| id.to_hex()),
            post_id: msg.post_id,
            sender_id: msg.sender_id,
            receiver_id: msg.receiver_id,
            sender_name: msg.sender_name,
            receiver_name: msg.receiver_name,  // ADDED THIS FIELD
            message: msg.message,
            seen: msg.seen,
            created_at: chrono_dt.to_rfc3339(),
            formatted_time,
        }
    }
}

// ========== ERROR RESPONSE ==========

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

// ========== HEALTH CHECK RESPONSE ==========

#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub service: String,
    pub timestamp: String,
}
