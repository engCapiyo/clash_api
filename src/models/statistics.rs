use bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStatistics {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "match_id")]
    pub match_id: String,
    pub minute: i32,
    #[serde(rename = "minute_display")]
    pub minute_display: String,
    #[serde(rename = "home_score")]
    pub home_score: i32,
    #[serde(rename = "away_score")]
    pub away_score: i32,
    #[serde(rename = "ball_possession_home")]
    pub ball_possession_home: i32,
    #[serde(rename = "ball_possession_away")]
    pub ball_possession_away: i32,
    #[serde(rename = "total_shots_home")]
    pub total_shots_home: i32,
    #[serde(rename = "total_shots_away")]
    pub total_shots_away: i32,
    #[serde(rename = "shots_on_target_home")]
    pub shots_on_target_home: i32,
    #[serde(rename = "shots_on_target_away")]
    pub shots_on_target_away: i32,
    #[serde(rename = "corners_home")]
    pub corners_home: i32,
    #[serde(rename = "corners_away")]
    pub corners_away: i32,
    #[serde(rename = "fouls_home")]
    pub fouls_home: i32,
    #[serde(rename = "fouls_away")]
    pub fouls_away: i32,
    #[serde(rename = "offsides_home")]
    pub offsides_home: i32,
    #[serde(rename = "offsides_away")]
    pub offsides_away: i32,
    #[serde(rename = "yellow_cards_home")]
    pub yellow_cards_home: i32,
    #[serde(rename = "yellow_cards_away")]
    pub yellow_cards_away: i32,
    #[serde(rename = "red_cards_home")]
    pub red_cards_home: i32,
    #[serde(rename = "red_cards_away")]
    pub red_cards_away: i32,
    #[serde(rename = "pass_accuracy_home")]
    pub pass_accuracy_home: i32,
    #[serde(rename = "pass_accuracy_away")]
    pub pass_accuracy_away: i32,
    #[serde(rename = "created_at")]
    pub created_at: BsonDateTime,
}

// Request struct from poller
#[derive(Debug, Deserialize)]
pub struct StatisticsRequest {
    pub match_id: String,
    pub minute: i32,
    pub minute_display: String,
    pub home_score: i32,
    pub away_score: i32,
    pub ball_possession_home: i32,
    pub ball_possession_away: i32,
    pub total_shots_home: i32,
    pub total_shots_away: i32,
    pub shots_on_target_home: i32,
    pub shots_on_target_away: i32,
    pub corners_home: i32,
    pub corners_away: i32,
    pub fouls_home: i32,
    pub fouls_away: i32,
    pub offsides_home: i32,
    pub offsides_away: i32,
    pub yellow_cards_home: i32,
    pub yellow_cards_away: i32,
    pub red_cards_home: i32,
    pub red_cards_away: i32,
    pub pass_accuracy_home: i32,
    pub pass_accuracy_away: i32,
    pub timestamp: Option<String>,
}

impl MatchStatistics {
    pub fn from_request(req: StatisticsRequest) -> Self {
        Self {
            id: format!("stats_{}_{}", req.match_id, req.minute),
            match_id: req.match_id,
            minute: req.minute,
            minute_display: req.minute_display,
            home_score: req.home_score,
            away_score: req.away_score,
            ball_possession_home: req.ball_possession_home,
            ball_possession_away: req.ball_possession_away,
            total_shots_home: req.total_shots_home,
            total_shots_away: req.total_shots_away,
            shots_on_target_home: req.shots_on_target_home,
            shots_on_target_away: req.shots_on_target_away,
            corners_home: req.corners_home,
            corners_away: req.corners_away,
            fouls_home: req.fouls_home,
            fouls_away: req.fouls_away,
            offsides_home: req.offsides_home,
            offsides_away: req.offsides_away,
            yellow_cards_home: req.yellow_cards_home,
            yellow_cards_away: req.yellow_cards_away,
            red_cards_home: req.red_cards_home,
            red_cards_away: req.red_cards_away,
            pass_accuracy_home: req.pass_accuracy_home,
            pass_accuracy_away: req.pass_accuracy_away,
            created_at: BsonDateTime::from_chrono(chrono::Utc::now()),
        }
    }
}
