use bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};

// ========== VOTER STRUCT ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voter {
    #[serde(rename = "userId")]
    pub user_id: String,

    #[serde(rename = "userName")]
    pub user_name: String,

    #[serde(rename = "selection")]
    pub selection: String,

    #[serde(rename = "isCorrect", skip_serializing_if = "Option::is_none")]
    pub is_correct: Option<bool>,

    #[serde(rename = "pointsAwarded", skip_serializing_if = "Option::is_none")]
    pub points_awarded: Option<i32>,

    #[serde(rename = "votedAt")]
    pub voted_at: BsonDateTime,
}

// ========== COMMENTARY ENTRY STRUCT ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentaryEntry {
    pub minute: i32,
    pub minute_display: String,
    pub text: String,
    pub event_type: String,
    pub home_score: i32,
    pub away_score: i32,
    pub team: Option<String>,   // Team that committed the action
    pub player: Option<String>, // Player involved
    pub created_at: BsonDateTime,
}

// ========== MAIN GAME MODEL - MATCHES DATABASE EXACTLY ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    #[serde(rename = "_id")]
    pub id: String,

    #[serde(rename = "match_id")]
    pub match_id: String,

    #[serde(rename = "sofascore_id", skip_serializing_if = "Option::is_none")]
    pub sofascore_id: Option<i64>,

    #[serde(rename = "home_team")]
    pub home_team: String,

    #[serde(rename = "away_team")]
    pub away_team: String,

    #[serde(rename = "league")]
    pub league: String,

    #[serde(rename = "home_win")]
    pub home_win: f64,

    #[serde(rename = "away_win")]
    pub away_win: f64,

    #[serde(rename = "draw")]
    pub draw: f64,

    #[serde(rename = "date")]
    pub date: String,

    #[serde(rename = "time")]
    pub time: String,

    #[serde(rename = "date_iso")]
    pub date_iso: String,

    #[serde(rename = "home_score", skip_serializing_if = "Option::is_none")]
    pub home_score: Option<i32>,

    #[serde(rename = "away_score", skip_serializing_if = "Option::is_none")]
    pub away_score: Option<i32>,

    #[serde(rename = "status")]
    pub status: String,

    #[serde(rename = "is_live")]
    pub is_live: bool,

    #[serde(rename = "available_for_voting")]
    pub available_for_voting: bool,

    #[serde(rename = "time_elapsed", default)]
    pub time_elapsed: i32,

    #[serde(rename = "result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>, // "home", "away", "draw"

    #[serde(rename = "source")]
    pub source: String,

    #[serde(rename = "scraped_at")]
    pub scraped_at: BsonDateTime,

    #[serde(rename = "votes", default)]
    pub votes: i64,

    #[serde(rename = "comments", default)]
    pub comments: i64,

    #[serde(rename = "voters", default)]
    pub voters: Vec<Voter>,

    // ========== COMMENTARY FIELDS ==========
    #[serde(default)]
    pub commentary: Vec<CommentaryEntry>,

    #[serde(default)]
    pub commentary_count: i64,

    pub last_commentary_at: Option<BsonDateTime>,
}

// ========== REQUEST STRUCTS ==========
#[derive(Debug, Deserialize)]
pub struct CreateGame {
    pub match_id: String,
    pub home_team: String,
    pub away_team: String,
    pub league: String,
    pub home_win: f64,
    pub away_win: f64,
    pub draw: f64,
    pub date: String,
    pub time: String,
    pub date_iso: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGameScore {
    pub match_id: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub status: Option<String>,
    pub is_live: Option<bool>,
    pub time_elapsed: Option<i32>,
}

// ========== LIVE GAME UPDATE FROM PYTHON POLLER ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveGameUpdate {
    pub fixture_id: String,
    pub event_type: String,
    pub home_score: i32,
    pub away_score: i32,
    pub minute: i32,
    pub minute_display: String,
    pub scorer: Option<String>,
    pub player: Option<String>,
    pub assist: Option<String>,
    pub team: Option<String>,
    pub player_out: Option<String>,
    pub player_in: Option<String>,
    pub on_target: Option<bool>,
    pub blocked: Option<bool>,
    #[serde(skip_deserializing)]
    pub timestamp: Option<BsonDateTime>,
}

// ========== COMMENTARY UPDATE FROM PYTHON POLLER ==========
#[derive(Debug, Deserialize)]
pub struct CommentaryUpdate {
    pub match_id: String,
    pub entry: CommentaryEntry,
}

#[derive(Debug, Deserialize)]
pub struct LatestCommentaryQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GameQuery {
    pub status: Option<String>,
    pub league: Option<String>,
    pub is_live: Option<bool>,
    pub limit: Option<i64>,
    pub skip: Option<u64>,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GameStatusUpdate {
    pub match_id: String,
    pub status: String,
    pub is_live: bool,
}

// ========== RESPONSE WRAPPERS ==========
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedGames {
    pub games: Vec<Game>,
    pub total: i64,
    pub page: u64,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct LiveGamesResponse {
    pub live_games: Vec<Game>,
    pub count: usize,
    pub last_updated: BsonDateTime,
}

// ========== DEFAULT IMPLEMENTATIONS ==========
impl Default for CommentaryEntry {
    fn default() -> Self {
        Self {
            minute: 0,
            minute_display: String::new(),
            text: String::new(),
            event_type: String::new(),
            home_score: 0,
            away_score: 0,
            team: None,
            player: None,
            created_at: BsonDateTime::from_chrono(chrono::Utc::now()),
        }
    }
}

impl LiveGameUpdate {
    pub fn into_event_request(self) -> EventRequest {
        EventRequest {
            match_id: self.fixture_id,
            event_type: self.event_type,
            minute: self.minute,
            minute_display: self.minute_display,
            home_score: self.home_score,
            away_score: self.away_score,
            player: self.player.or(self.scorer),
            team: self.team,
            assist: self.assist,
            player_out: self.player_out,
            player_in: self.player_in,
            shot_type: None,
            on_target: self.on_target,
            blocked: self.blocked,
        }
    }
}

// ============================================================================
// HISTORY GAME MODEL (for completed games archive)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryGame {
    #[serde(flatten)]
    pub game: Game,
    pub completed_at: BsonDateTime,
    pub moved_to_history: bool,
}

// ========== EVENT REQUEST (for compatibility with events handler) ==========
#[derive(Debug, Deserialize)]
pub struct EventRequest {
    pub match_id: String,
    pub event_type: String,
    pub minute: i32,
    pub minute_display: String,
    pub home_score: i32,
    pub away_score: i32,
    pub player: Option<String>,
    pub team: Option<String>,
    pub assist: Option<String>,
    pub player_out: Option<String>,
    pub player_in: Option<String>,
    pub shot_type: Option<String>,
    pub on_target: Option<bool>,
    pub blocked: Option<bool>,
}
