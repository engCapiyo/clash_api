// handlers/b2c_handlers.rs
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tracing::{info, error};

use crate::state::AppState;

// B2C Request
#[derive(Debug, Deserialize)]
pub struct B2CRequest {
    pub phone_number: String,
    pub amount: String,
    pub command_id: String,
    pub remarks: String,
    pub occasion: Option<String>,
}

// B2C Response
#[derive(Debug, Serialize)]
pub struct B2CResponse {
    pub success: bool,
    pub conversation_id: String,
    pub originator_conversation_id: String,
    pub response_code: String,
    pub response_description: String,
    pub message: String,
}

// B2C Callback
#[derive(Debug, Deserialize)]
pub struct B2CCallback {
    pub Result: B2CResult,
}

#[derive(Debug, Deserialize)]
pub struct B2CResult {
    #[serde(rename = "ResultType")]
    pub result_type: i32,

    #[serde(rename = "ResultCode")]
    pub result_code: i32,

    #[serde(rename = "ResultDesc")]
    pub result_desc: String,

    #[serde(rename = "OriginatorConversationID")]
    pub originator_conversation_id: String,

    #[serde(rename = "ConversationID")]
    pub conversation_id: String,

    #[serde(rename = "TransactionID")]
    pub transaction_id: String,

    #[serde(rename = "ResultParameters")]
    pub result_parameters: B2CResultParameters,
}

#[derive(Debug, Deserialize)]
pub struct B2CResultParameters {
    #[serde(rename = "ResultParameter")]
    pub result_parameter: Vec<B2CResultParameter>,
}

#[derive(Debug, Deserialize)]
pub struct B2CResultParameter {
    #[serde(rename = "Key")]
    pub key: String,

    #[serde(rename = "Value")]
    pub value: serde_json::Value,
}

// B2C Handlers
pub async fn send_b2c_payment(
    State(state): State<AppState>,
    Json(request): Json<B2CRequest>,
) -> impl IntoResponse {
    info!("Received B2C request: {:?}", request);

    let valid_commands = ["BusinessPayment", "SalaryPayment", "PromotionPayment"];
    if !valid_commands.contains(&request.command_id.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Invalid command_id. Must be: BusinessPayment, SalaryPayment, or PromotionPayment")
            }))
        );
    }

    let mpesa_service = match &state.mpesa_service {
        Some(service) => service,
        None => {
            error!("M-Pesa service not available");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "success": false,
                    "error": "M-Pesa service is not available"
                }))
            );
        }
    };

    match mpesa_service.send_b2c_payment(
        &request.phone_number,
        &request.amount,
        &request.command_id,
        &request.remarks,
        request.occasion.as_deref(),
    ).await {
        Ok(response) => {
            info!("B2C payment initiated: {}", response.conversation_id);

            let api_response = B2CResponse {
                success: true,
                conversation_id: response.conversation_id,
                originator_conversation_id: response.originator_conversation_id,
                response_code: response.response_code,
                response_description: response.response_description,
                message: "B2C payment initiated successfully".to_string(),
            };

            (StatusCode::OK, Json(serde_json::json!(api_response)))
        }
        Err(e) => {
            error!("Failed to send B2C payment: {}", e);

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": e.to_string()
                }))
            )
        }
    }
}

pub async fn b2c_result_callback(
    Json(callback): Json<B2CCallback>,
) -> impl IntoResponse {
    info!("Received B2C result callback: {:?}", callback.Result);

    if callback.Result.result_code == 0 {
        info!("B2C payment successful: {}", callback.Result.transaction_id);
    } else {
        error!("B2C payment failed: {} - {}", callback.Result.result_code, callback.Result.result_desc);
    }

    Json(serde_json::json!({
        "ResultCode": 0,
        "ResultDesc": "Success"
    }))
}

pub async fn b2c_timeout_callback(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    info!("Received B2C timeout callback: {:?}", payload);

    Json(serde_json::json!({
        "ResultCode": 0,
        "ResultDesc": "Success"
    }))
}

pub async fn get_b2c_status() -> impl IntoResponse {
    Json(serde_json::json!({
        "success": true,
        "count": 0,
        "transactions": []
    }))
}