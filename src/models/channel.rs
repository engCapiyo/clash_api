use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Add this new struct
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingRequest {
    pub user_id: String,
    pub username: String,
    pub requested_at: DateTime,
}

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

    pub invite_code: String,
    pub pending_requests: Vec<PendingRequest>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelMember {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub joined_at: DateTime,
    pub season_points: i32, // Denormalized from User
    pub correct_votes: i32, // Denormalized from User
    pub total_votes: i32,   // Denormalized from User
    pub msg_count: i32,

    // NEW: stamped whenever this member votes or sends a message in this
    // channel. Lets us compute "active in last N days" without a full
    // event log per member. None = never active since joining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<DateTime>,
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

    pub comment_count: i32,
    pub unread_counts: HashMap<String, i32>,

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

// ============================================================================
// MESSAGE MODEL
// ============================================================================

// ============================================================================
// MESSAGE MODEL
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    // ✅ FIX: Use skip_serializing_if to omit field when None
    #[serde(skip_serializing_if = "Option::is_none")]
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
// VOTE MODEL - NO channel_id (Global Vote)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Vote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
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

    // NEW: snapshot of cumulative totals at the moment this payout was
    // computed. Lets the next payout diff against these instead of
    // re-paying the same lifetime votes/messages every period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes_at_payout: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_at_payout: Option<i32>,
}

// ============================================================================
// MEMBERSHIP EVENT LOG - NEW
// ============================================================================
// Separate collection ("channel_membership_events"). Without this, you can't
// compute churn or net growth over a rolling window — `member_count` only
// ever tells you the current total, not who left or when. Every join/leave
// (and join-request approve/reject if you want that funnel too) writes one
// row here. Cheap to query: count "left" events in last 30d per channel_id.

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelMembershipEvent {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub user_id: String,
    pub event_type: String, // "joined" | "left" | "removed"
    pub occurred_at: DateTime,
}

// ============================================================================
// ADMIN REWARD SCORE - NEW
// ============================================================================
// Stores the computed score per channel per period, so you're not
// recalculating from scratch every time someone views a leaderboard, and so
// you have history to show admins ("your score last week vs this week").
// admin_user_id is denormalized from Channel.created_by / role=="admin" at
// computation time, since multi-admin channels are possible.

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AdminRewardScore {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub admin_user_id: String,
    pub period_start: DateTime,
    pub period_end: DateTime,

    // raw inputs, kept alongside the score so the formula can change later
    // without losing the ability to recompute history
    pub active_member_ratio: f64, // active_in_period / member_count
    pub vote_participation: f64,  // members who voted / member_count
    pub retention_rate: f64,      // members from period_start still here at period_end
    pub net_member_growth: i32,   // joins - leaves in period

    pub score: f64,
    pub computed_at: DateTime,
}
