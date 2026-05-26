// src/models/transaction.rs
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
    pub status: String,
    pub result_code: Option<i32>,
    pub result_desc: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpesaTransaction {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub paying_phone_number: String,
    pub transaction_date: String,
    pub mpesa_receipt_number: String,
    pub paid_amount: String,
    pub merchant_request_id: String,
    pub checkout_request_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateMpesaTransaction {
    pub paying_phone_number: String,
    pub transaction_date: String,
    pub mpesa_receipt_number: String,
    pub paid_amount: String,
    pub merchant_request_id: String,
    pub checkout_request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MpesaTransactionQuery {
    pub paying_phone_number: Option<String>,
    pub mpesa_receipt_number: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackData {
    #[allow(non_snake_case)]
    pub Body: CallbackBody,
}

#[derive(Debug, Deserialize)]
pub struct CallbackBody {
    #[allow(non_snake_case)]
    pub stkCallback: StkCallback,
}

#[derive(Debug, Deserialize)]
pub struct StkCallback {
    #[serde(rename = "MerchantRequestID")]
    pub merchant_request_id: String,
    #[serde(rename = "CheckoutRequestID")]
    pub checkout_request_id: String,
    #[serde(rename = "ResultCode")]
    pub result_code: i32,
    #[serde(rename = "ResultDesc")]
    pub result_desc: String,
    #[serde(rename = "CallbackMetadata", default)]
    pub callback_metadata: Option<CallbackMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackMetadata {
    #[allow(non_snake_case)]
    pub Item: Vec<MetadataItem>,
}

#[derive(Debug, Deserialize)]
pub struct MetadataItem {
    #[allow(non_snake_case)]
    pub Name: String,
    #[allow(non_snake_case)]
    pub Value: serde_json::Value,
}
