use bson::{oid::ObjectId, DateTime as BsonDateTime};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

// ============================================================================
// REPLY DATA STRUCT - For reply functionality (NEW)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyData {
    #[serde(rename = "messageId")]
    pub message_id: String,

    pub text: String,
    pub username: String,

    #[serde(rename = "selection", skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,

    #[serde(rename = "isMe")]
    pub is_me: bool,
}

// ========== VOTE MODELS ==========

// Vote model for storing votes
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Vote {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "homeTeam")]
    #[validate(length(min = 1, message = "Home team is required"))]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    #[validate(length(min = 1, message = "Away team is required"))]
    pub away_team: String,

    pub draw: String,

    #[serde(rename = "selection")]
    #[validate(length(min = 1, message = "Selection is required"))]
    pub selection: String,

    #[serde(rename = "voteTimestamp")]
    pub vote_timestamp: BsonDateTime,

    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<BsonDateTime>,
}

// For creating new votes (from Flutter app)
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateVote {
    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "awayTeam")]
    #[validate(length(min = 1, message = "Away team is required"))]
    pub away_team: String,

    #[serde(rename = "draw")]
    #[validate(length(min = 1, message = "Draw field is required"))]
    pub draw: String,

    #[serde(rename = "homeTeam")]
    #[validate(length(min = 1, message = "Home team is required"))]
    pub home_team: String,

    #[serde(rename = "selection")]
    #[validate(length(min = 1, message = "Selection is required"))]
    pub selection: String,
}

// ========== LIKE MODELS ==========

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Like {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "action")]
    #[validate(length(min = 1, message = "Action is required"))]
    pub action: String,

    #[serde(rename = "likeTimestamp")]
    pub like_timestamp: BsonDateTime,

    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<BsonDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateLike {
    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "action")]
    #[validate(length(min = 1, message = "Action is required"))]
    pub action: String,
}

// ============================================================================
// COMMENT MODELS - UPDATED WITH MEDIA & REPLY (NEW FIELDS)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Comment {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "selection")]
    #[validate(length(min = 1, message = "Selection is required"))]
    pub selection: String,

    #[serde(rename = "comment")]
    #[validate(length(max = 500, message = "Comment must be less than 500 characters"))]
    // ✅ FIXED
    pub comment: String,

    #[serde(rename = "timestamp")]
    pub timestamp: String,

    #[serde(rename = "commentTimestamp")]
    pub comment_timestamp: BsonDateTime,

    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<BsonDateTime>,

    #[serde(rename = "likes", skip_serializing_if = "Option::is_none")]
    pub likes: Option<i32>,

    #[serde(rename = "replies", skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<ObjectId>>,

    #[serde(rename = "seenBy", default)]
    pub seen_by: Vec<String>,

    #[serde(rename = "imageUrl", skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,

    #[serde(rename = "videoUrl", skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,

    #[serde(rename = "isImage", default)]
    pub is_image: bool,

    #[serde(rename = "isVideo", default)]
    pub is_video: bool,

    #[serde(rename = "replyTo", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyData>,
}
// ========== CREATE COMMENT REQUEST - UPDATED ==========

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateComment {
    #[serde(rename = "voterId")]
    #[validate(length(min = 1, message = "Voter ID is required"))]
    pub voter_id: String,

    #[serde(rename = "username")]
    #[validate(length(min = 1, message = "Username is required"))]
    pub username: String,

    #[serde(rename = "fixtureId")]
    #[validate(length(min = 1, message = "Fixture ID is required"))]
    pub fixture_id: String,

    #[serde(rename = "selection")]
    #[validate(length(min = 1, message = "Selection is required"))]
    pub selection: String,

    #[serde(rename = "comment")]
    // No validate here since comment can be empty if image/video present
    pub comment: String,

    #[serde(rename = "timestamp")]
    #[validate(length(min = 1, message = "Timestamp is required"))]
    pub timestamp: String,

    // NEW FIELDS
    #[serde(rename = "messageId", skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    #[serde(rename = "imageUrl", skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,

    #[serde(rename = "videoUrl", skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,

    #[serde(rename = "isImage", default)]
    pub is_image: bool,

    #[serde(rename = "isVideo", default)]
    pub is_video: bool,

    #[serde(rename = "replyTo", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyData>,
}
// ========== MARK COMMENTS AS SEEN REQUEST ==========

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct MarkCommentsSeenRequest {
    #[serde(rename = "userId")]
    pub user_id: String,

    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "commentIds", skip_serializing_if = "Option::is_none")]
    pub comment_ids: Option<Vec<String>>,
}

// ========== TYPING INDICATOR (WebSocket) ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingIndicator {
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub is_typing: bool,
}

// ========== READ RECEIPT (WebSocket) ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadReceipt {
    pub comment_id: String,
    pub user_id: String,
    pub username: String,
    pub fixture_id: String,
    pub seen_at: String,
}

// ========== UNREAD COUNTS RESPONSE ==========

#[derive(Debug, Serialize)]
pub struct UnreadCountsResponse {
    pub success: bool,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "unreadCounts")]
    pub unread_counts: std::collections::HashMap<String, i64>,
}

// ========== STATISTICS MODELS ==========

#[derive(Debug, Serialize, Deserialize)]
pub struct VoteStats {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    #[serde(rename = "totalVotes")]
    pub total_votes: i64,

    #[serde(rename = "homeVotes")]
    pub home_votes: i64,

    #[serde(rename = "drawVotes")]
    pub draw_votes: i64,

    #[serde(rename = "awayVotes")]
    pub away_votes: i64,

    #[serde(rename = "homePercentage")]
    pub home_percentage: f64,

    #[serde(rename = "drawPercentage")]
    pub draw_percentage: f64,

    #[serde(rename = "awayPercentage")]
    pub away_percentage: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LikeStats {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "totalLikes")]
    pub total_likes: i64,

    #[serde(rename = "userHasLiked")]
    pub user_has_liked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentStats {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "totalComments")]
    pub total_comments: i64,

    #[serde(rename = "homeComments")]
    pub home_comments: i64,

    #[serde(rename = "drawComments")]
    pub draw_comments: i64,

    #[serde(rename = "awayComments")]
    pub away_comments: i64,

    #[serde(rename = "recentComments")]
    pub recent_comments: Vec<Comment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentWithUser {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "voterId")]
    pub voter_id: String,

    #[serde(rename = "username")]
    pub username: String,

    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "selection")]
    pub selection: String,

    pub comment: String,
    pub timestamp: String,

    #[serde(rename = "commentTimestamp")]
    pub comment_timestamp: BsonDateTime,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<i32>,

    #[serde(rename = "replies", skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<ObjectId>>,

    #[serde(rename = "seenBy", default)]
    pub seen_by: Vec<String>,

    #[serde(rename = "userDisplayName", skip_serializing_if = "Option::is_none")]
    pub user_display_name: Option<String>,

    #[serde(rename = "userAvatar", skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,

    #[serde(rename = "isVerified")]
    pub is_verified: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserVoteStatus {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "hasVoted")]
    pub has_voted: bool,

    #[serde(rename = "voteSelection")]
    pub vote_selection: Option<String>,

    #[serde(rename = "hasLiked")]
    pub has_liked: bool,

    #[serde(rename = "userCommentsCount")]
    pub user_comments_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FixtureStats {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    #[serde(rename = "voteStats")]
    pub vote_stats: VoteStats,

    #[serde(rename = "likeStats")]
    pub like_stats: LikeStats,

    #[serde(rename = "commentStats")]
    pub comment_stats: CommentStats,
}

// ========== API RESPONSE WRAPPERS ==========

#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub success: bool,
    pub message: String,

    #[serde(rename = "voteId", skip_serializing_if = "Option::is_none")]
    pub vote_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vote>,
}

#[derive(Debug, Serialize)]
pub struct LikeResponse {
    pub success: bool,
    pub message: String,

    #[serde(rename = "likeId", skip_serializing_if = "Option::is_none")]
    pub like_id: Option<String>,

    #[serde(rename = "totalLikes")]
    pub total_likes: i64,
}

#[derive(Debug, Serialize)]
pub struct CommentResponse {
    pub success: bool,
    pub message: String,

    #[serde(rename = "commentId", skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<Comment>,
}

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub success: bool,
    pub data: FixtureStats,
}

// ========== QUERY PARAMETERS ==========

#[derive(Debug, Deserialize)]
pub struct VoteQuery {
    #[serde(rename = "fixtureId")]
    pub fixture_id: Option<String>,

    #[serde(rename = "voterId")]
    pub voter_id: Option<String>,
    pub limit: Option<i64>,
    pub skip: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CommentQuery {
    #[serde(rename = "fixtureId")]
    pub fixture_id: Option<String>,

    #[serde(rename = "voterId")]
    pub voter_id: Option<String>,

    #[serde(rename = "selection")]
    pub selection: Option<String>,

    pub limit: Option<i64>,
    pub skip: Option<u64>,

    #[serde(rename = "sortBy")]
    pub sort_by: Option<String>,
}

// ========== BULK OPERATIONS ==========

#[derive(Debug, Deserialize)]
pub struct BulkVoteRequest {
    pub votes: Vec<CreateVote>,
}

#[derive(Debug, Serialize)]
pub struct BulkVoteResponse {
    pub success: bool,

    #[serde(rename = "insertedCount")]
    pub inserted_count: u64,

    #[serde(rename = "failedCount")]
    pub failed_count: u64,

    #[serde(rename = "failedVotes")]
    pub failed_votes: Vec<FailedVote>,
}

#[derive(Debug, Serialize)]
pub struct FailedVote {
    pub index: usize,
    pub error: String,

    #[serde(rename = "voteData")]
    pub vote_data: CreateVote,
}

// ========== TOTAL COUNTS MODELS ==========

#[derive(Debug, Serialize, Deserialize)]
pub struct TotalCounts {
    #[serde(rename = "totalVotes")]
    pub total_votes: i64,

    #[serde(rename = "totalLikes")]
    pub total_likes: i64,

    #[serde(rename = "totalComments")]
    pub total_comments: i64,

    #[serde(rename = "totalEngagement")]
    pub total_engagement: i64,

    #[serde(rename = "totalUsers")]
    pub total_users: i64,

    #[serde(rename = "timestamp")]
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct TotalCountsResponse {
    pub success: bool,
    pub message: String,
    pub data: TotalCounts,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FixtureCounts {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    #[serde(rename = "totalVotes")]
    pub total_votes: i64,

    #[serde(rename = "homeVotes")]
    pub home_votes: i64,

    #[serde(rename = "drawVotes")]
    pub draw_votes: i64,

    #[serde(rename = "awayVotes")]
    pub away_votes: i64,

    #[serde(rename = "totalLikes")]
    pub total_likes: i64,

    #[serde(rename = "totalComments")]
    pub total_comments: i64,

    #[serde(rename = "totalEngagement")]
    pub total_engagement: i64,

    #[serde(rename = "userHasVoted")]
    pub user_has_voted: bool,

    #[serde(rename = "userHasLiked")]
    pub user_has_liked: bool,

    #[serde(rename = "userSelection")]
    pub user_selection: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FixtureCountsResponse {
    pub success: bool,
    pub message: String,
    pub data: FixtureCounts,
}

#[derive(Debug, Deserialize)]
pub struct BatchFixtureCountsRequest {
    #[serde(rename = "fixtureIds")]
    pub fixture_ids: Vec<String>,

    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FixtureCountItem {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    #[serde(rename = "totalVotes")]
    pub total_votes: i64,

    #[serde(rename = "totalLikes")]
    pub total_likes: i64,

    #[serde(rename = "totalComments")]
    pub total_comments: i64,

    #[serde(rename = "totalEngagement")]
    pub total_engagement: i64,

    #[serde(rename = "userHasVoted", skip_serializing_if = "Option::is_none")]
    pub user_has_voted: Option<bool>,

    #[serde(rename = "userHasLiked", skip_serializing_if = "Option::is_none")]
    pub user_has_liked: Option<bool>,

    #[serde(rename = "userSelection", skip_serializing_if = "Option::is_none")]
    pub user_selection: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchFixtureCountsResponse {
    pub success: bool,
    pub message: String,
    pub data: Vec<FixtureCountItem>,
    pub count: usize,
}

// ============================================================================
// VALIDATION HELPER
// ============================================================================

pub fn validate_selection(selection: &str) -> Result<(), String> {
    let valid_selections = vec!["home_team", "draw", "away_team"];
    if !valid_selections.contains(&selection) {
        return Err(format!("Selection must be one of: {:?}", valid_selections));
    }
    Ok(())
}

// ============================================================================
// TIMESTAMP PARSING
// ============================================================================

pub fn parse_iso_timestamp(timestamp_str: &str) -> Result<BsonDateTime, String> {
    println!("🔍 Parsing timestamp: '{}'", timestamp_str);

    if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp_str) {
        println!("✅ Parsed as RFC 3339: {}", dt);
        return Ok(BsonDateTime::from_millis(dt.timestamp_millis()));
    }

    if timestamp_str.ends_with('Z') {
        let without_z = timestamp_str.trim_end_matches('Z');

        if let Ok(ndt) = NaiveDateTime::parse_from_str(without_z, "%Y-%m-%dT%H:%M:%S%.f") {
            let dt_utc = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
            println!("✅ Parsed with milliseconds: {}", dt_utc);
            return Ok(BsonDateTime::from_millis(dt_utc.timestamp_millis()));
        }

        if let Ok(ndt) = NaiveDateTime::parse_from_str(without_z, "%Y-%m-%dT%H:%M:%S") {
            let dt_utc = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
            println!("✅ Parsed without milliseconds: {}", dt_utc);
            return Ok(BsonDateTime::from_millis(dt_utc.timestamp_millis()));
        }
    }

    if let Ok(dt) = DateTime::parse_from_str(timestamp_str, "%Y-%m-%dT%H:%M:%S%.f%:z") {
        println!("✅ Parsed with timezone offset: {}", dt);
        return Ok(BsonDateTime::from_millis(dt.timestamp_millis()));
    }

    if let Ok(dt) = DateTime::parse_from_str(timestamp_str, "%Y-%m-%dT%H:%M:%S%:z") {
        println!("✅ Parsed with timezone offset (no ms): {}", dt);
        return Ok(BsonDateTime::from_millis(dt.timestamp_millis()));
    }

    let dt_clean = timestamp_str.replace('T', " ").replace('Z', "");

    if let Ok(ndt) = NaiveDateTime::parse_from_str(&dt_clean, "%Y-%m-%d %H:%M:%S%.f") {
        let dt_utc = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
        println!("✅ Parsed as space-separated with ms: {}", dt_utc);
        return Ok(BsonDateTime::from_millis(dt_utc.timestamp_millis()));
    }

    if let Ok(ndt) = NaiveDateTime::parse_from_str(&dt_clean, "%Y-%m-%d %H:%M:%S") {
        let dt_utc = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
        println!("✅ Parsed as space-separated: {}", dt_utc);
        return Ok(BsonDateTime::from_millis(dt_utc.timestamp_millis()));
    }

    if let Ok(ts) = timestamp_str.parse::<i64>() {
        if timestamp_str.len() == 10 {
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                println!("✅ Parsed as Unix seconds: {}", dt);
                return Ok(BsonDateTime::from_millis(dt.timestamp_millis()));
            }
        } else if timestamp_str.len() >= 13 {
            return Ok(BsonDateTime::from_millis(ts));
        }
    }

    if let Ok(ndt) = NaiveDate::parse_from_str(timestamp_str, "%Y-%m-%d") {
        let dt_utc = ndt.and_hms_opt(0, 0, 0).unwrap().and_utc();
        println!("⚠️ Parsed as date only: {}", dt_utc);
        return Ok(BsonDateTime::from_millis(dt_utc.timestamp_millis()));
    }

    println!("❌ Failed to parse timestamp: '{}'", timestamp_str);
    Err(format!("Invalid timestamp format: '{}'", timestamp_str))
}

pub fn parse_iso_timestamp_or_now(timestamp_str: &str) -> BsonDateTime {
    parse_iso_timestamp(timestamp_str).unwrap_or_else(|_| {
        println!(
            "⚠️ Failed to parse timestamp '{}', using current time",
            timestamp_str
        );
        BsonDateTime::from_millis(Utc::now().timestamp_millis())
    })
}

// ============================================================================
// FROM CREATE IMPLEMENTATIONS
// ============================================================================

impl Vote {
    pub fn from_create_vote(create_vote: CreateVote) -> Self {
        Vote {
            id: None,
            voter_id: create_vote.voter_id,
            username: create_vote.username,
            fixture_id: create_vote.fixture_id,
            home_team: create_vote.home_team,
            away_team: create_vote.away_team,
            draw: create_vote.draw,
            selection: create_vote.selection,
            vote_timestamp: BsonDateTime::from_millis(Utc::now().timestamp_millis()),
            created_at: Some(BsonDateTime::from_millis(Utc::now().timestamp_millis())),
        }
    }
}

impl Like {
    pub fn from_create_like(create_like: CreateLike) -> Self {
        let current_time = BsonDateTime::from_millis(Utc::now().timestamp_millis());
        println!(
            "📝 Creating like for fixture: {}, user: {}, action: {}",
            create_like.fixture_id, create_like.voter_id, create_like.action
        );
        Like {
            id: None,
            voter_id: create_like.voter_id,
            username: create_like.username,
            fixture_id: create_like.fixture_id,
            action: create_like.action,
            like_timestamp: current_time,
            created_at: Some(current_time),
        }
    }
}

// ============================================================================
// COMMENT IMPLEMENTATION - UPDATED
// ============================================================================

impl Comment {
    pub fn from_create_comment(create_comment: CreateComment) -> Result<Self, String> {
        // Validate selection
        let valid_selections = vec!["home_team", "draw", "away_team"];
        if !valid_selections.contains(&create_comment.selection.as_str()) {
            return Err(format!(
                "Invalid selection: {}. Must be one of: home_team, draw, away_team",
                create_comment.selection
            ));
        }

        // Validate that at least comment, image, or video is provided
        let has_content = !create_comment.comment.is_empty()
            || create_comment.is_image
            || create_comment.is_video;

        if !has_content {
            return Err("Comment, image, or video is required".to_string());
        }

        let comment_timestamp = match parse_iso_timestamp(&create_comment.timestamp) {
            Ok(ts) => ts,
            Err(e) => {
                println!("⚠️ Timestamp parsing failed: {}, using current time", e);
                BsonDateTime::from_millis(Utc::now().timestamp_millis())
            }
        };

        Ok(Comment {
            id: None,
            voter_id: create_comment.voter_id,
            username: create_comment.username,
            fixture_id: create_comment.fixture_id,
            selection: create_comment.selection,
            comment: create_comment.comment,
            timestamp: create_comment.timestamp,
            comment_timestamp,
            created_at: Some(BsonDateTime::from_millis(Utc::now().timestamp_millis())),
            likes: Some(0),
            replies: Some(Vec::new()),
            seen_by: vec![],
            image_url: create_comment.image_url,
            video_url: create_comment.video_url,
            is_image: create_comment.is_image,
            is_video: create_comment.is_video,
            reply_to: create_comment.reply_to,
            message_id: create_comment.message_id,
        })
    }

    /// Check if a user has seen this comment
    pub fn is_seen_by(&self, user_id: &str) -> bool {
        self.seen_by.contains(&user_id.to_string())
    }

    /// Mark comment as seen by a user
    pub fn mark_seen(&mut self, user_id: &str) {
        let user_id_str = user_id.to_string();
        if !self.seen_by.contains(&user_id_str) {
            self.seen_by.push(user_id_str);
        }
    }
}

// ============================================================================
// SERIALIZATION HELPERS
// ============================================================================

pub fn bson_datetime_to_iso_string(dt: &BsonDateTime) -> String {
    let millis = dt.timestamp_millis();
    let dt_chrono = Utc
        .timestamp_millis_opt(millis)
        .single()
        .unwrap_or_else(|| Utc::now());
    dt_chrono.to_rfc3339()
}

pub fn option_bson_datetime_to_iso_string(dt: &Option<BsonDateTime>) -> Option<String> {
    dt.as_ref().map(bson_datetime_to_iso_string)
}

// ============================================================================
// RESPONSE WRAPPERS
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: u64,
    pub limit: i64,
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
        ErrorResponse {
            success: false,
            error: error.to_string(),
            message: message.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

// ============================================================================
// DEFAULT IMPLEMENTATIONS
// ============================================================================

impl Default for VoteResponse {
    fn default() -> Self {
        Self {
            success: false,
            message: String::new(),
            vote_id: None,
            data: None,
        }
    }
}

impl Default for LikeResponse {
    fn default() -> Self {
        Self {
            success: false,
            message: String::new(),
            like_id: None,
            total_likes: 0,
        }
    }
}

impl Default for CommentResponse {
    fn default() -> Self {
        Self {
            success: false,
            message: String::new(),
            comment_id: None,
            comment: None,
        }
    }
}
