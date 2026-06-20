use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub username: String,
    pub phone: String,
    pub balance: f64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    #[serde(default)]
    pub is_admin: bool,

    // GLOBAL POINTS FIELDS
    #[serde(default)]
    pub season_points: i32,
    #[serde(default)]
    pub correct_votes: i32,
    #[serde(default)]
    pub total_votes: i32,

    // PIN AUTH FIELDS (new) — defaulted so legacy documents without
    // these keys at all still deserialize successfully.
    #[serde(default)]
    pub pin_hash: Option<String>,
    #[serde(default)]
    pub pin_salt: Option<String>,
    #[serde(default)]
    pub is_pin_enabled: bool,
    #[serde(default)]
    pub firebase_uid: Option<String>,
    #[serde(default)]
    pub auth_methods: Vec<String>,
    #[serde(default)]
    pub last_login: Option<DateTime>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub phone: String,
}

#[derive(Debug, Deserialize)]
pub struct PinLoginRequest {
    pub phone: String,
    pub pin: String,
}

#[derive(Debug, Deserialize)]
pub struct SetPinRequest {
    pub pin: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub phone: String,
    pub balance: f64,
    pub is_admin: bool,
    pub season_points: i32,
    pub correct_votes: i32,
    pub total_votes: i32,
    pub has_pin: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub phone: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserPointsRequest {
    pub season_points: i32,
    pub correct_votes: i32,
    pub total_votes: i32,
}
