use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

// ============================================================================
// CHANNEL MODELS
// ============================================================================

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

// ============================================================================
// FIXTURE CHAT MODELS
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelFixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub fixture_id: String,
    pub match_name: String,
    pub kickoff_time: DateTime,
    pub status: String,
    pub vote_counts: VoteCounts,
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
// MESSAGE MODELS
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
}

// ============================================================================
// VOTE MODELS
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
// PAYOUT MODELS
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
