use chrono::{DateTime, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,

    pub user_id: String,
    pub phone_number: String,
    pub amount: f64,
    pub merchant_request_id: String,
    pub checkout_request_id: String,
    pub response_code: String,
    pub response_description: String,
    pub customer_message: String,
    pub status: String, // "pending", "completed", "failed"
    pub result_code: Option<i32>,
    pub result_desc: Option<String>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    // FIXED: For Option<DateTime<Utc>>, use a different approach
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,

    pub mpesa_receipt: Option<String>,
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
