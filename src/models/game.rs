use chrono::{DateTime, Utc};
use mongodb::bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// VOTER
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voter {
    #[serde(rename = "userId")]
    pub user_id: String,

    #[serde(rename = "userName")]
    pub user_name: String,

    pub selection: String, // "home", "away", "draw"

    #[serde(rename = "isCorrect", skip_serializing_if = "Option::is_none")]
    pub is_correct: Option<bool>,

    #[serde(rename = "pointsAwarded", skip_serializing_if = "Option::is_none")]
    pub points_awarded: Option<i32>,

    #[serde(rename = "votedAt")]
    pub voted_at: BsonDateTime,
}

// ============================================================================
// COMMENTARY ENTRY
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentaryEntry {
    pub minute: i32,
    pub text: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub team: Option<String>,
    pub player: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: BsonDateTime,
}

// ============================================================================
// STATISTICS - MATCHES PYTHON POLLER AND FLUTTER
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamStatistics {
    pub possession: Option<f64>,
    pub shots: Option<i32>,
    #[serde(rename = "shotsOnTarget")]
    pub shots_on_target: Option<i32>,
    #[serde(rename = "shotsOffTarget")]
    pub shots_off_target: Option<i32>,
    pub corners: Option<i32>,
    pub fouls: Option<i32>,
    #[serde(rename = "yellowCards")]
    pub yellow_cards: Option<i32>,
    #[serde(rename = "redCards")]
    pub red_cards: Option<i32>,
    pub offsides: Option<i32>,
    pub passes: Option<i32>,
    #[serde(rename = "passAccuracy")]
    pub pass_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStatistics {
    pub home: TeamStatistics,
    pub away: TeamStatistics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticsSnapshot {
    pub minute: i32,
    pub statistics: MatchStatistics,
    pub timestamp: BsonDateTime,
}

// ============================================================================
// LINEUPS - MATCHES PYTHON POLLER AND FLUTTER
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: String, // "starting" or "bench"
    #[serde(rename = "playerId")]
    pub player_id: Option<String>,
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coach {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamLineup {
    pub formation: String,
    pub coach: Coach,
    pub players: Vec<Player>,
    pub bench: Vec<Player>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineupsDocument {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "matchId")]
    pub match_id: String,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    #[serde(rename = "homeLineup")]
    pub home_lineup: TeamLineup,

    #[serde(rename = "awayLineup")]
    pub away_lineup: TeamLineup,

    #[serde(rename = "fetchedAt")]
    pub fetched_at: BsonDateTime,
}

// ============================================================================
// FLEXIBLE NUMBER DESERIALIZATION
// Some legacy/malformed documents in Mongo have score fields stored as
// doubles (e.g. 3.0) instead of int32, which causes strict i32 deserialize
// to fail with "invalid type: floating point `3.0`, expected i32".
// This lets us tolerate + coerce those without needing to patch the DB.
// ============================================================================
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum FlexibleInt {
    Int(i32),
    Float(f64),
}

impl FlexibleInt {
    fn into_i32(self) -> i32 {
        match self {
            FlexibleInt::Int(i) => i,
            FlexibleInt::Float(f) => f.round() as i32,
        }
    }
}

fn deserialize_flexible_opt_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<FlexibleInt>::deserialize(deserializer)?;
    Ok(opt.map(FlexibleInt::into_i32))
}

// ============================================================================
// MAIN GAME MODEL - MATCHES PYTHON SCRAPER AND FLUTTER
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "matchId")]
    pub match_id: String,

    #[serde(rename = "threesixtyfiveGameId")]
    #[serde(default)]
    pub threesixtyfive_game_id: Option<String>,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    pub league: String,

    #[serde(rename = "minutesPlayed")]
    #[serde(default)]
    pub minutes_played: Option<i32>,

    #[serde(rename = "minuteDisplay")]
    #[serde(default)]
    pub minute_display: Option<String>,

    #[serde(rename = "homeWin")]
    #[serde(default)]
    pub home_win: Option<f64>,

    #[serde(rename = "awayWin")]
    #[serde(default)]
    pub away_win: Option<f64>,

    #[serde(default)]
    pub draw: Option<f64>,

    pub date: String,
    pub time: String,

    #[serde(rename = "dateIso")]
    pub date_iso: String,

    #[serde(rename = "kickoffUtc")]
    pub kickoff_utc: DateTime<Utc>,

    #[serde(rename = "homeScore")]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_flexible_opt_i32")]
    pub home_score: Option<i32>,

    #[serde(rename = "awayScore")]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_flexible_opt_i32")]
    pub away_score: Option<i32>,

    pub status: String,

    #[serde(rename = "isLive")]
    pub is_live: bool,

    #[serde(rename = "availableForVoting")]
    pub available_for_voting: bool,

    #[serde(rename = "timeElapsed")]
    #[serde(default)]
    pub time_elapsed: Option<f64>,

    #[serde(default)]
    pub result: Option<String>,

    pub source: String,

    #[serde(rename = "scrapedAt")]
    pub scraped_at: BsonDateTime,

    #[serde(rename = "lastScrapedAt")]
    #[serde(default)]
    pub last_scraped_at: Option<BsonDateTime>,

    #[serde(rename = "lastPolledAt")]
    #[serde(default)]
    pub last_polled_at: Option<BsonDateTime>,

    pub votes: i64,
    pub voters: Vec<Voter>,

    pub comments: i64,
    pub commentary: Vec<CommentaryEntry>,

    #[serde(rename = "commentaryCount")]
    pub commentary_count: i64,

    #[serde(rename = "lastCommentaryAt")]
    #[serde(default)]
    pub last_commentary_at: Option<BsonDateTime>,

    #[serde(default)]
    pub lineups: Option<LineupsDocument>,

    #[serde(rename = "lineupsFetched")]
    pub lineups_fetched: bool,

    #[serde(rename = "lineupsFetchedAt")]
    #[serde(default)]
    pub lineups_fetched_at: Option<BsonDateTime>,

    pub statistics: Vec<StatisticsSnapshot>,

    #[serde(rename = "lastStatisticsMinute")]
    #[serde(default)]
    pub last_statistics_minute: Option<f64>,

    #[serde(rename = "forwardedEventSignatures")]
    pub forwarded_event_signatures: Vec<String>,

    #[serde(rename = "completedAt")]
    #[serde(default)]
    pub completed_at: Option<BsonDateTime>,

    #[serde(rename = "movedToHistory")]
    pub moved_to_history: bool,

    #[serde(rename = "createdAt")]
    pub created_at: BsonDateTime,
}

// ============================================================================
// HISTORY GAME - FOR games_history COLLECTION
// ============================================================================
// ============================================================================
// HISTORY GAME - FOR games_history COLLECTION (FLAT STRUCTURE)
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryGame {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "matchId")]
    pub match_id: String,

    #[serde(rename = "threesixtyfiveGameId")]
    #[serde(default)]
    pub threesixtyfive_game_id: Option<String>,

    #[serde(rename = "homeTeam")]
    pub home_team: String,

    #[serde(rename = "awayTeam")]
    pub away_team: String,

    pub league: String,

    #[serde(rename = "minutesPlayed")]
    #[serde(default)]
    pub minutes_played: Option<i32>,

    #[serde(rename = "minuteDisplay")]
    #[serde(default)]
    pub minute_display: Option<String>,

    #[serde(rename = "homeWin")]
    #[serde(default)]
    pub home_win: Option<f64>,

    #[serde(rename = "awayWin")]
    #[serde(default)]
    pub away_win: Option<f64>,

    #[serde(default)]
    pub draw: Option<f64>,

    pub date: String,
    pub time: String,

    #[serde(rename = "dateIso")]
    pub date_iso: String,

    #[serde(rename = "kickoffUtc")]
    pub kickoff_utc: DateTime<Utc>,

    // ✅ ADD flexible deserialization here
    #[serde(rename = "homeScore")]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_flexible_opt_i32")]
    pub home_score: Option<i32>,

    #[serde(rename = "awayScore")]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_flexible_opt_i32")]
    pub away_score: Option<i32>,

    pub status: String,

    #[serde(rename = "isLive")]
    pub is_live: bool,

    #[serde(rename = "availableForVoting")]
    pub available_for_voting: bool,

    #[serde(rename = "timeElapsed")]
    #[serde(default)]
    pub time_elapsed: Option<f64>,

    #[serde(default)]
    pub result: Option<String>,

    pub source: String,

    #[serde(rename = "scrapedAt")]
    pub scraped_at: BsonDateTime,

    #[serde(rename = "lastScrapedAt")]
    #[serde(default)]
    pub last_scraped_at: Option<BsonDateTime>,

    #[serde(rename = "lastPolledAt")]
    #[serde(default)]
    pub last_polled_at: Option<BsonDateTime>,

    pub votes: i64,
    pub voters: Vec<Voter>,

    pub comments: i64,
    pub commentary: Vec<CommentaryEntry>,

    #[serde(rename = "commentaryCount")]
    pub commentary_count: i64,

    #[serde(rename = "lastCommentaryAt")]
    #[serde(default)]
    pub last_commentary_at: Option<BsonDateTime>,

    #[serde(default)]
    pub lineups: Option<LineupsDocument>,

    #[serde(rename = "lineupsFetched")]
    pub lineups_fetched: bool,

    #[serde(rename = "lineupsFetchedAt")]
    #[serde(default)]
    pub lineups_fetched_at: Option<BsonDateTime>,

    pub statistics: Vec<StatisticsSnapshot>,

    #[serde(rename = "lastStatisticsMinute")]
    #[serde(default)]
    pub last_statistics_minute: Option<f64>,

    #[serde(rename = "forwardedEventSignatures")]
    pub forwarded_event_signatures: Vec<String>,

    #[serde(rename = "completedAt")]
    pub completed_at: BsonDateTime,

    #[serde(rename = "movedToHistory")]
    pub moved_to_history: bool,

    #[serde(rename = "createdAt")]
    pub created_at: BsonDateTime,
}
#[derive(Debug, Deserialize)]
pub struct UpdateGameScoreRequest {
    #[serde(rename = "matchId")]
    pub match_id: String,
    #[serde(rename = "homeScore")]
    pub home_score: Option<i32>,
    #[serde(rename = "awayScore")]
    pub away_score: Option<i32>,
    pub status: Option<String>,
    #[serde(rename = "isLive")]
    pub is_live: Option<bool>,
    #[serde(rename = "timeElapsed")]
    pub time_elapsed: Option<f64>,
    #[serde(rename = "minutesPlayed")]
    pub minutes_played: Option<i32>,
    #[serde(rename = "minuteDisplay")]
    pub minute_display: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GameQuery {
    pub status: Option<String>,
    pub league: Option<String>,
    #[serde(rename = "isLive")]
    pub is_live: Option<bool>,
    pub limit: Option<i64>,
    pub skip: Option<u64>,
    pub source: Option<String>,
}

// ============================================================================
// LIVE UPDATE FROM POLLER
// ============================================================================
#[derive(Debug, Clone, Deserialize)]
pub struct LiveGameUpdate {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    #[serde(rename = "homeScore")]
    pub home_score: i32,
    #[serde(rename = "awayScore")]
    pub away_score: i32,
    pub minute: i32,
    #[serde(rename = "minuteDisplay")]
    pub minute_display: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "isLive")]
    pub is_live: Option<bool>,
    #[serde(rename = "availableForVoting")]
    pub available_for_voting: Option<bool>,
    pub scorer: Option<String>,
    pub player: Option<String>,
    pub assist: Option<String>,
    pub team: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(rename = "minutesPlayed")]
    pub minutes_played: Option<i32>,
}

// ============================================================================
// LINEUPS UPDATE FROM POLLER
// ============================================================================
#[derive(Debug, Deserialize)]
pub struct LineupsUpdate {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,
    #[serde(rename = "homeTeam")]
    pub home_team: String,
    #[serde(rename = "awayTeam")]
    pub away_team: String,
    pub lineups: LineupsPayload,
}

#[derive(Debug, Deserialize)]
pub struct LineupsPayload {
    pub home: TeamLineupPayload,
    pub away: TeamLineupPayload,
}

#[derive(Debug, Deserialize)]
pub struct TeamLineupPayload {
    pub formation: String,
    pub coach: CoachPayload,
    pub players: Vec<PlayerPayload>,
    pub bench: Vec<PlayerPayload>,
}

#[derive(Debug, Deserialize)]
pub struct CoachPayload {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerPayload {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: String,
    #[serde(rename = "playerId")]
    pub player_id: Option<String>,
}

// ============================================================================
// STATISTICS UPDATE FROM POLLER
// ============================================================================
#[derive(Debug, Deserialize)]
pub struct StatisticsSnapshotPayload {
    pub fixture_id: String,
    pub minute: i32,
    pub statistics: MatchStatisticsPayload,
}

#[derive(Debug, Deserialize)]
pub struct MatchStatisticsPayload {
    pub home: TeamStatisticsPayload,
    pub away: TeamStatisticsPayload,
}

#[derive(Debug, Deserialize)]
pub struct TeamStatisticsPayload {
    pub possession: Option<f64>,
    pub shots: Option<i32>,
    pub shots_on_target: Option<i32>,
    pub shots_off_target: Option<i32>,
    pub corners: Option<i32>,
    pub fouls: Option<i32>,
    pub yellow_cards: Option<i32>,
    pub red_cards: Option<i32>,
    pub offsides: Option<i32>,
    pub passes: Option<i32>,
    pub pass_accuracy: Option<f64>,
}

// ============================================================================
// EVENTS FROM POLLER
// ============================================================================
#[derive(Debug, Deserialize)]
pub struct EventRequest {
    #[serde(rename = "fixtureId")]
    pub fixture_id: String,
    #[serde(rename = "eventType")]
    pub event_type: String,
    pub minute: i32,
    pub team: String,
    pub player: String,
    pub assist: Option<String>,
    #[serde(rename = "homeScore")]
    pub home_score: i32,
    #[serde(rename = "awayScore")]
    pub away_score: i32,
}

// ============================================================================
// RESPONSE WRAPPERS
// ============================================================================
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

// ============================================================================
// DEFAULT IMPLEMENTATIONS
// ============================================================================
impl Default for TeamStatistics {
    fn default() -> Self {
        Self {
            possession: None,
            shots: None,
            shots_on_target: None,
            shots_off_target: None,
            corners: None,
            fouls: None,
            yellow_cards: None,
            red_cards: None,
            offsides: None,
            passes: None,
            pass_accuracy: None,
        }
    }
}

impl Default for MatchStatistics {
    fn default() -> Self {
        Self {
            home: TeamStatistics::default(),
            away: TeamStatistics::default(),
        }
    }
}

impl Default for CommentaryEntry {
    fn default() -> Self {
        Self {
            minute: 0,
            text: String::new(),
            event_type: String::new(),
            team: None,
            player: None,
            created_at: BsonDateTime::from_chrono(Utc::now()),
        }
    }
}

// ============================================================================
// CONSTRUCTORS
// ============================================================================
impl LineupsDocument {
    pub fn new(
        match_id: String,
        home_team: String,
        away_team: String,
        home_lineup: TeamLineup,
        away_lineup: TeamLineup,
    ) -> Self {
        Self {
            id: None,
            match_id,
            home_team,
            away_team,
            home_lineup,
            away_lineup,
            fetched_at: BsonDateTime::from_chrono(Utc::now()),
        }
    }
}

impl Game {
    pub fn new(
        match_id: String,
        home_team: String,
        away_team: String,
        league: String,
        date: String,
        time: String,
        date_iso: String,
        kickoff_utc: DateTime<Utc>,
        source: String,
    ) -> Self {
        let now = BsonDateTime::from_chrono(Utc::now());
        Self {
            id: None,
            match_id,
            threesixtyfive_game_id: None,
            home_team,
            away_team,
            league,
            minutes_played: None,
            minute_display: None,
            home_win: None,
            away_win: None,
            draw: None,
            date,
            time,
            date_iso,
            kickoff_utc,
            home_score: None,
            away_score: None,
            status: "upcoming".to_string(),
            is_live: false,
            available_for_voting: true,
            time_elapsed: None,
            result: None,
            source,
            scraped_at: now,
            last_scraped_at: Some(now),
            last_polled_at: None,
            votes: 0,
            voters: Vec::new(),
            comments: 0,
            commentary: Vec::new(),
            commentary_count: 0,
            last_commentary_at: None,
            lineups: None,
            lineups_fetched: false,
            lineups_fetched_at: None,
            statistics: Vec::new(),
            last_statistics_minute: None,
            forwarded_event_signatures: Vec::new(),
            completed_at: None,
            moved_to_history: false,
            created_at: now,
        }
    }
}
