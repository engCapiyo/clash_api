use bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};

// ========== EVENT MODEL (Single source of truth for match events) ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub match_id: String,
    pub event_type: String,
    pub minute: i32,
    pub minute_display: String,
    pub home_score: i32,
    pub away_score: i32,

    pub data: EventData,
    pub created_at: BsonDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub scorer: Option<String>,
    pub assist: Option<String>,
    pub team: Option<String>,
    pub player: Option<String>,
    pub card_type: Option<String>,
    pub player_out: Option<String>,
    pub player_in: Option<String>,
    pub shot_type: Option<String>,
    pub on_target: Option<bool>,
    pub blocked: Option<bool>,
    pub foul_player: Option<String>,
}

// ========== REQUEST FROM PYTHON POLLER ==========
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

impl Event {
    pub fn from_request(req: EventRequest) -> Self {
        let mut data = EventData {
            scorer: None,
            assist: None,
            team: None,
            player: None,
            card_type: None,
            player_out: None,
            player_in: None,
            shot_type: None,
            on_target: None,
            blocked: None,
            foul_player: None,
        };

        match req.event_type.as_str() {
            "goal" => {
                data.scorer = req.player.clone();
                data.assist = req.assist.clone();
                data.team = req.team.clone();
            }
            "yellow_card" => {
                data.player = req.player.clone();
                data.card_type = Some("yellow".to_string());
                data.team = req.team.clone();
            }
            "red_card" => {
                data.player = req.player.clone();
                data.card_type = Some("red".to_string());
                data.team = req.team.clone();
            }
            "substitution" => {
                data.player_out = req.player_out.clone();
                data.player_in = req.player_in.clone();
                data.team = req.team.clone();
            }
            "shot" => {
                data.player = req.player.clone();
                data.team = req.team.clone();
                data.shot_type = req.shot_type.clone();
                data.on_target = req.on_target;
                data.blocked = req.blocked;
            }
            "foul" => {
                data.foul_player = req.player.clone();
                data.team = req.team.clone();
            }
            "corner" | "offside" => {
                data.team = req.team.clone();
            }
            _ => {}
        }

        Self {
            id: None,
            match_id: req.match_id,
            event_type: req.event_type,
            minute: req.minute,
            minute_display: req.minute_display,
            home_score: req.home_score,
            away_score: req.away_score,
            data,
            created_at: BsonDateTime::from_chrono(chrono::Utc::now()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EventsResponse {
    pub success: bool,
    pub data: Vec<Event>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SingleEventResponse {
    pub success: bool,
    pub data: Option<Event>,
}
