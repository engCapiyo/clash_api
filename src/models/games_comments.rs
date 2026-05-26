//posterId
// postername
// postId
// poster_club
// comment
// time

use bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};

// Main Game model - matches your MongoDB documents EXACTLY
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct games_comments {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    #[serde(rename = "poster_id")]
    pub poster_id: String,

    #[serde(rename = "postername")]
    pub postername: String,
    #[serde(rename = "post_id")]
    pub post_id: String,

    #[serde(rename = "comment")]
    pub comment: String,

    #[serde(rename = "time")]
    pub time: String,

    #[serde(rename = "created_at", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<BsonDateTime>,
}

// For creating new comments
#[derive(Debug, Deserialize)]
pub struct CreateComment {
    pub poster_id: String,
    pub postername: String,
    pub post_id: String,
    pub comment: String,

    pub date: String,
    pub time: String,
}

// For updating game scores
/*#[derive(Debug, Deserialize)]
pub struct UpdateGameScore {
    pub match_id: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub status: Option<String>,
    pub is_live: Option<bool>,
}

// For live game updates
#[derive(Debug, Deserialize)]
pub struct LiveUpdate {
    pub match_id: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
}*/

// For query parameters
#[derive(Debug, Deserialize)]
pub struct CommentQuery {
    pub status: Option<String>,
    pub league: Option<String>,
    pub is_live: Option<bool>,
    pub limit: Option<i64>,
    pub skip: Option<u64>,
}

// Response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
    pub message: Option<String>,
}

// For paginated responses
#[derive(Debug, Serialize)]
pub struct PaginatedGames {
    pub games: Vec<Game>,
    pub total: i64,
    pub page: u64,
    pub limit: i64,
}

// For live games response
#[derive(Debug, Serialize)]
pub struct LiveGamesResponse {
    pub live_games: Vec<Game>,
    pub count: usize,
    pub last_updated: BsonDateTime,
}
