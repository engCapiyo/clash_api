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
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub phone: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub phone: String,
    pub balance: f64,
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
