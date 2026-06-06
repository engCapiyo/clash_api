use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: DateTime,
    pub members: Vec<ChannelMember>,
    pub activity: ChannelActivity,
    pub season: String,
    pub member_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelMember {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub joined_at: DateTime,
    pub season_points: i32,
    pub correct_votes: i32,
    pub total_votes: i32,
    pub msg_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelActivity {
    pub total_messages: i32,
    pub messages_this_week: i32,
    pub week_reset_at: DateTime,
    pub last_message_at: Option<DateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Fixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "match_id")]
    pub fixture_id: String,
    pub home_team: String,
    pub away_team: String,
    #[serde(rename = "date_iso")]
    pub date_iso: String,
    #[serde(rename = "time")]
    pub time: String,
    pub status: String,
    pub result: Option<String>,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelFixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub fixture_id: String,
    pub match_name: String,
    pub kickoff_time: String,
    pub status: String,
    pub vote_counts: VoteCounts,

    // NEW FIELDS FOR COMMENT TRACKING
    pub comment_count: i32,                  // Total comments
    pub unread_counts: HashMap<String, i32>, // Per-user unread counts

    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime>,
    pub last_sender: Option<String>,
    pub added_at: DateTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoteCounts {
    pub home: i32,
    pub away: i32,
    pub draw: i32,
}

// ============================================================================
// REPLY DATA
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplyToData {
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub text: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(rename = "isMe")]
    pub is_me: bool,
}

// ============================================================================
// MESSAGE MODEL
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub fixture_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    pub sent_at: DateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(default)]
    pub is_image: bool,
    #[serde(default)]
    pub is_video: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyToData>,
}

// ============================================================================
// VOTE MODEL
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Vote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub fixture_id: String,
    pub user_id: String,
    pub selection: String,
    pub is_correct: Option<bool>,
    pub points_awarded: Option<i32>,
    pub voted_at: DateTime,
}

// ============================================================================
// PAYOUT MODEL
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Payout {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub channel_id: String,
    pub payout_type: String,
    pub amount: f64,
    pub currency: String,
    pub week: Option<i32>,
    pub season: String,
    pub status: String,
    pub created_at: DateTime,
    pub paid_at: Option<DateTime>,
}
