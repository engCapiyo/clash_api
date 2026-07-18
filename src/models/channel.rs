use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// PENDING REQUEST
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingRequest {
    pub user_id: String,
    pub username: String,
    pub requested_at: DateTime,
}

// ============================================================================
// CHANNEL
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
    pub invite_code: String,
    pub pending_requests: Vec<PendingRequest>,
}

// ============================================================================
// CHANNEL MEMBER
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChannelMember {
    pub user_id: String,
    pub username: String,
    pub role: String, // "admin" | "member"
    pub joined_at: DateTime,
    pub season_points: i32,
    pub correct_votes: i32,
    pub total_votes: i32,
    pub msg_count: i32,
    pub likes_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<DateTime>,
}

// ============================================================================
// CHANNEL ACTIVITY
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelActivity {
    pub total_messages: i32,
    pub messages_this_week: i32,
    pub week_reset_at: DateTime,
    pub last_message_at: Option<DateTime>,
}

// ============================================================================
// FIXTURE (Global - Master Data Only)
// ============================================================================

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
    // ✅ NO counts here — they belong in channel_fixtures
}

// ============================================================================
// VOTE COUNTS
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoteCounts {
    pub home: i32,
    pub away: i32,
    pub draw: i32,
}

impl VoteCounts {
    pub fn total(&self) -> i32 {
        self.home + self.away + self.draw
    }
}

// ============================================================================
// CHANNEL FIXTURE (Per-channel fixture data — ALL COUNTS LIVE HERE)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelFixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub fixture_id: String,
    pub match_name: String,
    pub kickoff_time: String,
    pub status: String,

    // ✅ ALL COUNTS LIVE HERE
    pub vote_counts: VoteCounts,
    pub comment_count: i32,
    pub pledge_count: i32, // ✅ NEW
    pub bet_count: i32,    // ✅ NEW
    pub likes_count: i32,

    pub unread_counts: HashMap<String, i32>,
    pub last_message: Option<String>,
    pub last_message_at: Option<DateTime>,
    pub last_sender: Option<String>,
    pub added_at: DateTime,
}

// ============================================================================
// MESSAGE
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
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

// ============================================================================
// REPLY TO DATA
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
// VOTE (Global - No channel_id)
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct Vote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub fixture_id: String,
    pub user_id: String,
    pub user_name: String,
    pub selection: String, // "home" | "away" | "draw"
    pub is_correct: Option<bool>,
    pub points_awarded: Option<i32>,
    pub voted_at: DateTime,
}

impl Vote {
    pub fn new(fixture_id: String, user_id: String, user_name: String, selection: String) -> Self {
        Self {
            id: None,
            fixture_id,
            user_id,
            user_name,
            selection,
            is_correct: None,
            points_awarded: None,
            voted_at: DateTime::now(),
        }
    }
}

// ============================================================================
// PAYOUT
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
    pub status: String, // "pending" | "paid"
    pub created_at: DateTime,
    pub paid_at: Option<DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes_at_payout: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_at_payout: Option<i32>,
}

// ============================================================================
// MEMBERSHIP EVENT LOG
// ============================================================================

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
// ADMIN REWARD SCORE
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AdminRewardScore {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    pub admin_user_id: String,
    pub period_start: DateTime,
    pub period_end: DateTime,
    pub active_member_ratio: f64,
    pub vote_participation: f64,
    pub retention_rate: f64,
    pub net_member_growth: i32,
    pub score: f64,
    pub computed_at: DateTime,
}

// ============================================================================
// LIKE
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Like {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub fixture_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub created_at: DateTime,
}

impl Like {
    pub fn new(fixture_id: String, channel_id: String, user_id: String, username: String) -> Self {
        Self {
            id: None,
            fixture_id,
            channel_id,
            user_id,
            username,
            created_at: DateTime::now(),
        }
    }
}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl ChannelMember {
    pub fn new(
        user_id: String,
        username: String,
        role: String,
        joined_at: DateTime,
        season_points: i32,
        correct_votes: i32,
        total_votes: i32,
    ) -> Self {
        Self {
            user_id,
            username,
            role,
            joined_at,
            season_points,
            correct_votes,
            total_votes,
            msg_count: 0,
            likes_count: 0,
            last_active_at: None,
        }
    }
}

impl ChannelFixture {
    pub fn new(
        channel_id: String,
        fixture_id: String,
        match_name: String,
        kickoff_time: String,
        status: String,
        members: &[ChannelMember],
    ) -> Self {
        let mut unread_counts = HashMap::new();
        for member in members {
            unread_counts.insert(member.user_id.clone(), 0);
        }

        Self {
            id: None,
            channel_id,
            fixture_id,
            match_name,
            kickoff_time,
            status,
            vote_counts: VoteCounts {
                home: 0,
                away: 0,
                draw: 0,
            },
            comment_count: 0,
            pledge_count: 0,
            bet_count: 0,
            likes_count: 0,
            unread_counts,
            last_message: None,
            last_message_at: None,
            last_sender: None,
            added_at: DateTime::now(),
        }
    }
}
