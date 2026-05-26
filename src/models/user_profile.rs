use serde::{Deserialize, Serialize};
use mongodb::bson::{oid::ObjectId, DateTime as BsonDateTime};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserProfile {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub user_id: String,
    pub username: String,
    pub phone: String,
    pub nickname: String,
    pub club_fan: String,
    pub country_fan: String,
    pub balance: f64,
    pub number_of_bets: i32,

    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserProfile {
    #[validate(length(min = 1))]
    pub user_id: String,

    pub username: String,

    #[validate(length(min = 10))]
    pub phone: String,

    pub nickname: String,
    pub club_fan: String,
    pub country_fan: String,

    #[validate(range(min = 0.0))]
    pub balance: f64,

    #[validate(range(min = 0))]
    pub number_of_bets: i32,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateBalanceRequest {
    #[validate(length(min = 1))]
    pub user_id: String,

    #[validate(range(min = 0.0))]
    pub balance: f64,
}

#[derive(Debug, Deserialize)]
pub struct UserQuery {
    pub user_id: Option<String>,
    pub phone: Option<String>,
    pub username: Option<String>,
}