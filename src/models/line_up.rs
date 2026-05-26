use bson::DateTime as BsonDateTime;
use serde::{Deserialize, Serialize};

// ========== MAIN LINEUPS STRUCT ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lineups {
    pub home: TeamLineup,
    pub away: TeamLineup,
}

// ========== TEAM LINEUP ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamLineup {
    pub formation: String,
    pub players: Vec<Player>,
    pub bench: Vec<Player>,
    pub coach: Coach,
}

// ========== PLAYER STRUCT ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
}

// ========== COACH STRUCT ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coach {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

// ========== FOR DATABASE STORAGE (Option 1: Embed in Game) ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLineups {
    #[serde(rename = "match_id")]
    pub match_id: String,
    pub home_team: String,
    pub away_team: String,
    pub home_formation: String,
    pub away_formation: String,
    pub home_coach: String,
    pub away_coach: String,
    pub home_starting_xi: Vec<Player>,
    pub home_bench: Vec<Player>,
    pub away_starting_xi: Vec<Player>,
    pub away_bench: Vec<Player>,
    pub fetched_at: BsonDateTime,
}

// ========== FOR DATABASE STORAGE (Option 2: Separate Collection) ==========
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineupsDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "match_id")]
    pub match_id: String,
    pub home_team: String,
    pub away_team: String,
    pub home_formation: String,
    pub away_formation: String,
    pub home_coach: String,
    pub away_coach: String,
    pub home_starting_xi: Vec<Player>,
    pub home_bench: Vec<Player>,
    pub away_starting_xi: Vec<Player>,
    pub away_bench: Vec<Player>,
    pub fetched_at: BsonDateTime,
}

// ========== FOR RECEIVING FROM POLLER ==========
#[derive(Debug, Deserialize)]
pub struct LineupsUpdate {
    pub fixture_id: String,
    pub lineups: RawLineupsData,
    pub timestamp: String,
}

#[derive(Debug, Deserialize)]
pub struct RawLineupsData {
    pub home: RawTeamLineup,
    pub away: RawTeamLineup,
}

#[derive(Debug, Deserialize)]
pub struct RawTeamLineup {
    pub formation: String,
    pub players: Vec<RawPlayer>,
    pub bench: Vec<RawPlayer>,
    pub coach: RawCoach,
}

#[derive(Debug, Deserialize)]
pub struct RawPlayer {
    pub name: String,
    pub position: String,
    #[serde(rename = "jerseyNumber")]
    pub jersey_number: i32,
    pub captain: bool,
    pub lineup: bool,
    #[serde(rename = "playerId")]
    pub player_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawCoach {
    pub name: String,
    pub country: Option<String>,
}

// ========== HELPER METHODS ==========
impl LineupsDocument {
    pub fn new(
        match_id: String,
        home_team: String,
        away_team: String,
        home_formation: String,
        away_formation: String,
        home_coach: String,
        away_coach: String,
        home_starting_xi: Vec<Player>,
        home_bench: Vec<Player>,
        away_starting_xi: Vec<Player>,
        away_bench: Vec<Player>,
    ) -> Self {
        Self {
            id: format!("lineup_{}", match_id),
            match_id,
            home_team,
            away_team,
            home_formation,
            away_formation,
            home_coach,
            away_coach,
            home_starting_xi,
            home_bench,
            away_starting_xi,
            away_bench,
            fetched_at: BsonDateTime::from_chrono(chrono::Utc::now()),
        }
    }

    pub fn get_home_starting_xi_names(&self) -> Vec<String> {
        self.home_starting_xi
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    pub fn get_away_starting_xi_names(&self) -> Vec<String> {
        self.away_starting_xi
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    pub fn get_home_captain(&self) -> Option<&Player> {
        self.home_starting_xi.iter().find(|p| p.captain)
    }

    pub fn get_away_captain(&self) -> Option<&Player> {
        self.away_starting_xi.iter().find(|p| p.captain)
    }

    pub fn get_player_by_number(&self, team: &str, number: i32) -> Option<&Player> {
        if team == "home" {
            self.home_starting_xi
                .iter()
                .find(|p| p.jersey_number == number)
        } else {
            self.away_starting_xi
                .iter()
                .find(|p| p.jersey_number == number)
        }
    }
}

impl Player {
    pub fn is_goalkeeper(&self) -> bool {
        self.position == "G"
    }

    pub fn is_defender(&self) -> bool {
        self.position == "D"
    }

    pub fn is_midfielder(&self) -> bool {
        self.position == "M"
    }

    pub fn is_forward(&self) -> bool {
        self.position == "F"
    }
}

// ========== FOR RESPONSES ==========
#[derive(Debug, Serialize)]
pub struct LineupsResponse {
    pub success: bool,
    pub data: LineupsDocument,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SimplifiedLineups {
    pub home_formation: String,
    pub away_formation: String,
    pub home_coach: String,
    pub away_coach: String,
    pub home_starting_xi: Vec<SimplifiedPlayer>,
    pub away_starting_xi: Vec<SimplifiedPlayer>,
}

#[derive(Debug, Serialize)]
pub struct SimplifiedPlayer {
    pub name: String,
    pub position: String,
    pub number: i32,
    pub captain: bool,
}

impl From<&Player> for SimplifiedPlayer {
    fn from(player: &Player) -> Self {
        Self {
            name: player.name.clone(),
            position: player.position.clone(),
            number: player.jersey_number,
            captain: player.captain,
        }
    }
}
