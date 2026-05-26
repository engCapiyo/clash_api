use serde::{Deserialize, Serialize};
use mongodb::bson::oid::ObjectId;
use chrono::{DateTime, Utc};
use mongodb::bson;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub amount: String,
    pub phone_number: String,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePayment {
    pub amount: String,
    pub phone_number: String,
}

#[derive(Debug, Deserialize)]
pub struct PaymentQuery {
    pub phone_number: Option<String>,
    pub amount: Option<String>,
}