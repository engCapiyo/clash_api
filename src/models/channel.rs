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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Fixture {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    #[serde(rename = "match_id")]
    pub fixture_id: String,
    pub home_team: String,
    pub away_team: String,
    pub league: String,
    #[serde(rename = "date_iso")]
    pub date_iso: String,
    #[serde(rename = "time")]
    pub time: String,
    pub status: String,
    pub result: Option<String>,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub home_win: f64,
    pub away_win: f64,
    pub draw: f64,
    pub is_live: bool,
    pub available_for_voting: bool,
    pub source: String,
    pub scraped_at: DateTime,
    pub time_elapsed: f64,
    pub sub_fixtures: Vec<SubFixture>,
}

// ============================================================================
// SUB FIXTURE (Prop Bets)
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubFixture {
    pub id: String,
    pub parent_fixture_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub icon: Option<String>,
    pub is_active: bool,
    pub vote_counts: VoteCounts,
    pub created_at: DateTime,
}

// ============================================================================
// VOTE COUNTS
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
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
    #[serde(default)]
    pub match_name: String,
    #[serde(default)]
    pub kickoff_time: String,
    pub status: String,

    pub vote_counts: VoteCounts,
    pub comment_count: i32,
    pub pledge_count: i32,
    pub bet_count: i32,
    pub likes_count: i32,

    #[serde(default)]
    pub unread_counts: HashMap<String, i32>,
    #[serde(default)]
    pub last_message: Option<String>,
    #[serde(default)]
    pub last_message_at: Option<DateTime>,
    #[serde(default)]
    pub last_sender: Option<String>,
    pub added_at: DateTime,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(default)]
    pub is_image: bool,
    #[serde(default)]
    pub is_video: bool,
}

// ============================================================================
// MESSAGE - FULL WITH CAPTION, IMAGES, VIDEOS, REPLY SUPPORT, temp_id
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,

    // Text content
    pub text: String,

    // Caption for media (separate from text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,

    pub sent_at: DateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,

    // ✅ NEW: temp_id for pending message tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_id: Option<String>,

    // Image fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_caption: Option<String>,
    #[serde(default)]
    pub is_image: bool,

    // Video fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    #[serde(default)]
    pub is_video: bool,

    // Reply support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyToData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
}

// ============================================================================
// MESSAGE REQUEST (For creating new messages)
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateMessageRequest {
    pub user_id: String,
    pub username: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    // ✅ NEW: temp_id for pending message tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_caption: Option<String>,
    #[serde(default)]
    pub is_image: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    #[serde(default)]
    pub is_video: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_selection: Option<String>,
}

// ============================================================================
// MESSAGE RESPONSE
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_id: Option<String>,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub sent_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    // ✅ NEW: temp_id for pending message tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temp_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_caption: Option<String>,
    pub is_image: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_public_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_size: Option<i64>,
    pub is_video: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyToData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
}

impl From<Message> for MessageResponse {
    fn from(msg: Message) -> Self {
        MessageResponse {
            id: msg.id.map(|id| id.to_hex()).unwrap_or_default(),
            channel_id: msg.channel_id,
            fixture_id: msg.fixture_id,
            sender_id: msg.sender_id,
            sender_name: msg.sender_name,
            text: msg.text,
            caption: msg.caption,
            sent_at: msg.sent_at.to_string(),
            message_id: msg.message_id,
            selection: msg.selection,
            temp_id: msg.temp_id, // ✅ Include temp_id in response
            image_url: msg.image_url,
            image_public_id: msg.image_public_id,
            image_caption: msg.image_caption,
            is_image: msg.is_image,
            video_url: msg.video_url,
            video_public_id: msg.video_public_id,
            video_thumbnail_url: msg.video_thumbnail_url,
            video_caption: msg.video_caption,
            video_duration: msg.video_duration,
            video_size: msg.video_size,
            is_video: msg.is_video,
            reply_to: msg.reply_to,
            reply_to_id: msg.reply_to_id,
        }
    }
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
// CHAT MEDIA UPLOAD REQUEST
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatMediaUploadRequest {
    pub file: Vec<u8>,
    pub file_name: String,
    pub mime_type: String,
    pub media_type: String, // "image" or "video"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

// ============================================================================
// CHAT MEDIA UPLOAD RESPONSE
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMediaUploadResponse {
    pub url: String,
    pub public_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

// ============================================================================
// PLEDGE
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pledge {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    pub username: String,
    pub phone: String,
    pub selection: String,
    pub amount: f64,
    pub time: chrono::DateTime<chrono::Utc>,
    pub fan: String,
    pub home_team: String,
    pub away_team: String,
    pub starter_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// GAME (Legacy support)
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Game {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub game_id: String,
    pub match_id: String,
    pub home_team: String,
    pub away_team: String,
    pub league: String,
    pub date_iso: String,
    pub time: String,
    pub status: String,
    pub result: Option<String>,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub home_win: f64,
    pub away_win: f64,
    pub draw: f64,
    pub is_live: bool,
    pub available_for_voting: bool,
    pub source: String,
    pub scraped_at: DateTime,
    pub time_elapsed: f64,
    pub sub_fixtures: Vec<SubFixture>,
}

// ============================================================================
// BET
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bet {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub fixture_id: String,
    pub starter_id: String,
    pub starter_name: String,
    pub starter_selection: String,
    pub starter_amount: f64,
    pub finisher_id: Option<String>,
    pub finisher_name: Option<String>,
    pub finisher_selection: Option<String>,
    pub finisher_amount: Option<f64>,
    pub vote_id: String,
    pub status: String, // "open" | "matched" | "settled"
    pub winner_id: Option<String>,
    pub starter_result: Option<String>,
    pub finisher_result: Option<String>,
    pub created_at: DateTime,
    pub matched_at: Option<DateTime>,
    pub settled_at: Option<DateTime>,
}

impl Bet {
    pub fn new_open(
        fixture_id: String,
        starter_id: String,
        starter_name: String,
        starter_selection: String,
        starter_amount: f64,
        vote_id: String,
    ) -> Self {
        Self {
            id: None,
            fixture_id,
            starter_id,
            starter_name,
            starter_selection,
            starter_amount,
            finisher_id: None,
            finisher_name: None,
            finisher_selection: None,
            finisher_amount: None,
            vote_id,
            status: "open".to_string(),
            winner_id: None,
            starter_result: None,
            finisher_result: None,
            created_at: DateTime::now(),
            matched_at: None,
            settled_at: None,
        }
    }

    pub fn total_pot(&self) -> f64 {
        self.starter_amount + self.finisher_amount.unwrap_or(0.0)
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

impl Message {
    pub fn new(channel_id: String, sender_id: String, sender_name: String, text: String) -> Self {
        let now = DateTime::now();
        Self {
            id: Some(ObjectId::new()),
            channel_id,
            fixture_id: None,
            sender_id,
            sender_name,
            text,
            caption: None,
            sent_at: now,
            message_id: None,
            selection: None,
            temp_id: None, // ✅ Initialize temp_id
            image_url: None,
            image_public_id: None,
            image_caption: None,
            is_image: false,
            video_url: None,
            video_public_id: None,
            video_thumbnail_url: None,
            video_caption: None,
            video_duration: None,
            video_size: None,
            is_video: false,
            reply_to: None,
            reply_to_id: None,
        }
    }

    pub fn with_image(
        channel_id: String,
        sender_id: String,
        sender_name: String,
        text: String,
        image_url: String,
        image_public_id: String,
        caption: Option<String>,
        temp_id: Option<String>,
    ) -> Self {
        let now = DateTime::now();
        Self {
            id: Some(ObjectId::new()),
            channel_id,
            fixture_id: None,
            sender_id,
            sender_name,
            text,
            caption: caption.clone(),
            sent_at: now,
            message_id: None,
            selection: None,
            temp_id,
            image_url: Some(image_url),
            image_public_id: Some(image_public_id),
            image_caption: caption,
            is_image: true,
            video_url: None,
            video_public_id: None,
            video_thumbnail_url: None,
            video_caption: None,
            video_duration: None,
            video_size: None,
            is_video: false,
            reply_to: None,
            reply_to_id: None,
        }
    }

    pub fn with_video(
        channel_id: String,
        sender_id: String,
        sender_name: String,
        text: String,
        video_url: String,
        video_public_id: String,
        video_thumbnail_url: Option<String>,
        caption: Option<String>,
        duration: Option<i32>,
        size: Option<i64>,
        temp_id: Option<String>,
    ) -> Self {
        let now = DateTime::now();
        Self {
            id: Some(ObjectId::new()),
            channel_id,
            fixture_id: None,
            sender_id,
            sender_name,
            text,
            caption: caption.clone(),
            sent_at: now,
            message_id: None,
            selection: None,
            temp_id,
            image_url: None,
            image_public_id: None,
            image_caption: None,
            is_image: false,
            video_url: Some(video_url),
            video_public_id: Some(video_public_id),
            video_thumbnail_url,
            video_caption: caption,
            video_duration: duration,
            video_size: size,
            is_video: true,
            reply_to: None,
            reply_to_id: None,
        }
    }

    pub fn with_reply(
        channel_id: String,
        sender_id: String,
        sender_name: String,
        text: String,
        reply_to_id: String,
        reply_to_text: String,
        reply_to_username: String,
        reply_to_selection: Option<String>,
        temp_id: Option<String>,
    ) -> Self {
        let now = DateTime::now();
        Self {
            id: Some(ObjectId::new()),
            channel_id,
            fixture_id: None,
            sender_id,
            sender_name,
            text,
            caption: None,
            sent_at: now,
            message_id: None,
            selection: None,
            temp_id,
            image_url: None,
            image_public_id: None,
            image_caption: None,
            is_image: false,
            video_url: None,
            video_public_id: None,
            video_thumbnail_url: None,
            video_caption: None,
            video_duration: None,
            video_size: None,
            is_video: false,
            reply_to: Some(ReplyToData {
                message_id: reply_to_id.clone(),
                text: reply_to_text,
                username: reply_to_username,
                selection: reply_to_selection,
                is_me: false,
                image_url: None,
                video_url: None,
                is_image: false,
                is_video: false,
            }),
            reply_to_id: Some(reply_to_id),
        }
    }
}
