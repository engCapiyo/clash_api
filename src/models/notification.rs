use serde::{Deserialize, Serialize};
use mongodb::bson::{doc, oid::ObjectId, DateTime as BsonDateTime};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FCMToken {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub fcm_token: String,
    pub platform: String, // "android" or "ios"
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterTokenRequest {
    pub user_id: String,
    pub fcm_token: String,
    pub platform: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notification {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub notification_type: String, // "vote_rival", "vote_supporter", "like", "comment"
    pub title: String,
    pub body: String,
    pub data: serde_json::Value,
    pub is_read: bool,
    pub created_at: BsonDateTime,
}

#[derive(Debug, Deserialize)]
pub struct SendNotificationRequest {
    pub user_id: String,
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize,Clone)]
pub struct NotificationResponse {
    pub success: bool,
    pub message: String,
    pub notification_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MarkReadRequest {
    pub user_id: String,
    pub notification_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct NotificationPreferences {
    pub vote_alerts: bool,
    pub like_alerts: bool,
    pub comment_alerts: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub user_id: String,
    pub vote_alerts: bool,
    pub like_alerts: bool,
    pub comment_alerts: bool,
}
