// src/handlers/mpesa_handlers.rs
use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::Json as AxumJson,
};
use chrono::Utc;
use futures_util::StreamExt;
use mongodb::bson::doc;
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::models::transaction::Transaction;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct StkPushRequest {
    pub phone_number: String,
    pub amount: String,
    pub account_reference: Option<String>,
    pub transaction_desc: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct ValidationRequest {
    // Can be empty - we don't need the data
}

#[derive(Debug, Deserialize)]
pub struct StatusRequest {
    pub checkout_request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub checkout_request_id: Option<String>,
    pub merchant_request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MpesaCallback {
    #[allow(non_snake_case)]
    pub Body: CallbackBody,
}

#[derive(Debug, Deserialize)]
pub struct CallbackBody {
    #[serde(rename = "stkCallback")]
    pub stk_callback: StkCallback,
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
    #[serde(rename = "CallbackMetadata")]
    pub callback_metadata: Option<CallbackMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackMetadata {
    #[serde(rename = "Item")]
    pub items: Vec<CallbackItem>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackItem {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Value")]
    pub value: serde_json::Value,
}

pub async fn mpesa_validation(
    Json(_payload): Json<serde_json::Value>, // Ignore the payload
) -> AxumJson<serde_json::Value> {
    println!("✅✅✅ [VALIDATION] Received from Safaricom");
    println!("✅✅✅ [VALIDATION] Auto-approving transaction");

    // ALWAYS return success immediately
    // This just tells Safaricom "yes, you can proceed with this transaction"
    AxumJson(json!({
        "ResultCode": 0,
        "ResultDesc": "Success"
    }))
}

// ✅ HANDLER 1: Initiate STK Push
pub async fn initiate_stk_push(
    State(state): State<AppState>,
    Json(request): Json<StkPushRequest>,
) -> Result<AxumJson<serde_json::Value>, (StatusCode, AxumJson<serde_json::Value>)> {
    println!("🔵 [STK] === INITIATING STK PUSH ===");
    println!("📱 Phone: {}", request.phone_number);
    println!("💰 Amount: {}", request.amount);
    println!("👤 User ID: {:?}", request.account_reference);

    if request.phone_number.is_empty() || request.amount.is_empty() {
        println!("❌ [STK] Validation failed: empty phone or amount");
        return Err((
            StatusCode::BAD_REQUEST,
            AxumJson(json!({
                "success": false,
                "error": "Phone number and amount are required"
            })),
        ));
    }

    let amount: f64 = match request.amount.parse() {
        Ok(amount) if amount > 0.0 => amount,
        _ => {
            println!("❌ [STK] Invalid amount: {}", request.amount);
            return Err((
                StatusCode::BAD_REQUEST,
                AxumJson(json!({
                    "success": false,
                    "error": "Amount must be greater than 0"
                })),
            ));
        }
    };

    let mpesa_service = match &state.mpesa_service {
        Some(service) => service,
        None => {
            println!("❌ [STK] M-Pesa service unavailable");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                AxumJson(json!({
                    "success": false,
                    "error": "M-Pesa service is not available"
                })),
            ));
        }
    };

    println!("✅ [STK] Calling M-Pesa service...");

    let response = match mpesa_service
        .initiate_stk_push(
            &request.phone_number,
            &request.amount,
            request.account_reference.as_deref(),
            request.transaction_desc.as_deref(),
        )
        .await
    {
        Ok(resp) => {
            println!("✅ [STK] M-Pesa response received");
            println!("🎫 MerchantRequestID: {}", resp.merchant_request_id);
            println!("🎫 CheckoutRequestID: {}", resp.checkout_request_id);
            println!("📝 Response: {}", resp.response_description);
            resp
        }
        Err(e) => {
            println!("❌ [STK] M-Pesa service error: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(json!({
                    "success": false,
                    "error": e.to_string()
                })),
            ));
        }
    };

    println!("💾 [STK] Saving transaction to database...");
    let transaction = Transaction {
        id: None,
        user_id: request
            .account_reference
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        phone_number: request.phone_number.clone(),
        amount,
        merchant_request_id: response.merchant_request_id.clone(),
        checkout_request_id: response.checkout_request_id.clone(),
        response_code: response.response_code.clone(),
        response_description: response.response_description.clone(),
        customer_message: response.customer_message.clone(),
        status: "pending".to_string(),
        result_code: None,
        result_desc: None,
        created_at: Utc::now().to_rfc3339(), // ✅ String
        updated_at: Utc::now().to_rfc3339(), // ✅ String
        completed_at: None,
    };

    let collection: Collection<Transaction> = state.db.collection("transactions");
    match collection.insert_one(&transaction).await {
        Ok(_) => println!("✅ [STK] Transaction saved to database"),
        Err(e) => println!("⚠️ [STK] Failed to save transaction: {}", e),
    };

    let api_response = json!({
        "success": true,
        "CheckoutRequestID": response.checkout_request_id,
        "checkout_request_id": response.checkout_request_id,
        "merchant_request_id": response.merchant_request_id,
        "response_code": response.response_code,
        "response_description": response.response_description,
        "customer_message": response.customer_message,
    });

    println!("✅ [STK] Returning response to client");
    println!("📤 Response: {:?}", api_response);
    println!("🟢 [STK] === STK PUSH COMPLETE ===");

    Ok(AxumJson(api_response))
}

// ✅ HANDLER 2: M-Pesa Callback
// ✅ HANDLER 2: M-Pesa Callback - FIXED VERSION
pub async fn mpesa_confirmation(
    State(state): State<AppState>,
    Json(payload): Json<MpesaCallback>,
) -> AxumJson<serde_json::Value> {
    println!("🎯 [CALLBACK RECEIVED] ==================================");
    println!("🎯 Timestamp: {}", Utc::now().to_rfc3339());
    println!(
        "🎯 MerchantRequestID: {}",
        payload.Body.stk_callback.merchant_request_id
    );
    println!(
        "🎯 CheckoutRequestID: {}",
        payload.Body.stk_callback.checkout_request_id
    );
    println!("🎯 ResultCode: {}", payload.Body.stk_callback.result_code);
    println!("🎯 ResultDesc: {}", payload.Body.stk_callback.result_desc);
    println!("🎯 ======================================================");

    info!("Received M-Pesa callback: {:?}", payload.Body.stk_callback);

    let callback = payload.Body.stk_callback;

    if callback.merchant_request_id.is_empty() || callback.checkout_request_id.is_empty() {
        error!("Invalid callback: missing required fields");
        return AxumJson(json!({
            "ResultCode": 1,
            "ResultDesc": "Invalid callback data"
        }));
    }

    let collection: Collection<Transaction> = state.db.collection("transactions");
    let checkout_id = callback.checkout_request_id.clone();

    // STEP 1: Find the transaction using ONLY checkout_request_id
    // merchant_request_id can vary in format, so don't use it for matching
    let filter = doc! {
        "checkout_request_id": &checkout_id,
    };

    match collection.find_one(filter).await {
        Ok(Some(transaction)) => {
            println!("✅ Found transaction in database");
            println!("📦 Transaction ID: {:?}", transaction.id);
            println!("📦 Current status: {}", transaction.status);

            // STEP 2: Get the MongoDB _id for reliable updating
            let id = match &transaction.id {
                Some(id) => {
                    println!("✅ Using _id for update: {:?}", id);
                    id
                }
                None => {
                    error!("❌ Transaction found but missing _id field!");
                    return AxumJson(json!({
                        "ResultCode": 1,
                        "ResultDesc": "Transaction missing ID field"
                    }));
                }
            };

            // STEP 3: Determine new status based on ResultCode
            let status = if callback.result_code == 0 {
                "completed"
            } else {
                "failed"
            };

            println!("🔄 Updating transaction status to: {}", status);

            // STEP 4: Extract amount from callback metadata if available
            let mut amount = 0.0;
            if let Some(metadata) = &callback.callback_metadata {
                for item in &metadata.items {
                    if item.name == "Amount" {
                        if let serde_json::Value::Number(num) = &item.value {
                            amount = num.as_f64().unwrap_or(0.0);
                            println!("💰 Amount from callback: Ksh {}", amount);
                        }
                        break;
                    }
                }
            }

            // STEP 5: Extract M-Pesa receipt number if available
            let mut mpesa_receipt = String::new();
            if let Some(metadata) = &callback.callback_metadata {
                for item in &metadata.items {
                    if item.name == "MpesaReceiptNumber" {
                        if let serde_json::Value::String(receipt) = &item.value {
                            mpesa_receipt = receipt.clone();
                            println!("🧾 Receipt: {}", mpesa_receipt);
                        }
                        break;
                    }
                }
            }

            // STEP 6: Update using the document's _id (guaranteed to work)
            let update = doc! {
                "$set": {
                    "status": status,
                    "result_code": callback.result_code,
                    "result_desc": &callback.result_desc,
                    "mpesa_receipt": mpesa_receipt,
                    "updated_at": Utc::now().to_rfc3339(),
                    "completed_at": Utc::now().to_rfc3339(),
                }
            };

            match collection.update_one(doc! { "_id": id }, update).await {
                Ok(result) => {
                    if result.matched_count > 0 {
                        println!(
                            "✅ SUCCESS: Updated transaction {} to {}",
                            checkout_id, status
                        );
                        info!(
                            "Updated transaction {} to {} using _id",
                            checkout_id, status
                        );

                        if callback.result_code == 0 {
                            info!(
                                "💰 Payment successful: Ksh {} for checkout {}",
                                amount, checkout_id
                            );
                            println!("🎉 Payment completed successfully!");
                        } else {
                            println!("❌ Payment failed: {}", callback.result_desc);
                        }
                    } else {
                        println!("⚠️ No document matched the _id! This should never happen.");
                        error!("Failed to update transaction: no document matched _id");
                    }
                }
                Err(e) => {
                    println!("❌ Database update error: {}", e);
                    error!("Failed to update transaction: {}", e);
                }
            }
        }
        Ok(None) => {
            println!("⚠️ Transaction NOT FOUND for checkout_id: {}", checkout_id);
            println!(
                "⚠️ This means the callback arrived but we have no record of this transaction!"
            );
            warn!("Transaction not found for callback: {}", checkout_id);
        }
        Err(e) => {
            println!("❌ Database query error: {}", e);
            error!("Failed to find transaction: {}", e);
        }
    }

    // STEP 7: Always return success to Safaricom (200 OK)
    // If we don't, Safaricom will keep retrying the callback
    println!("📤 Returning success response to Safaricom");
    println!("🎯 [CALLBACK COMPLETE] =================================");

    AxumJson(json!({
        "ResultCode": 0,
        "ResultDesc": "Success"
    }))
}

// ✅ HANDLER 3: Check Payment Status (POST - for frontend polling)
pub async fn check_payment_status(
    State(state): State<AppState>,
    Json(request): Json<StatusRequest>,
) -> (StatusCode, AxumJson<serde_json::Value>) {
    println!(
        "🔍 Checking payment status for: {}",
        request.checkout_request_id
    );

    let collection: Collection<Transaction> = state.db.collection("transactions");
    let filter = doc! { "checkout_request_id": &request.checkout_request_id };

    match collection.find_one(filter).await {
        Ok(Some(transaction)) => {
            let is_success = transaction.status == "completed";
            let is_failed = transaction.status == "failed";

            let response = json!({
                "success": is_success,
                "failed": is_failed,
                "status": transaction.status,
                "result_code": transaction.result_code,
                "result_desc": transaction.result_desc,
                "checkout_request_id": transaction.checkout_request_id,
                "merchant_request_id": transaction.merchant_request_id,
                "amount": transaction.amount,
                "phone_number": transaction.phone_number,
                "updated_at": transaction.updated_at,   // ✅ already a String
                "created_at": transaction.created_at,   // ✅ already a String
            });

            println!(
                "✅ Found transaction: {} (success: {}, failed: {})",
                transaction.status, is_success, is_failed
            );
            (StatusCode::OK, AxumJson(response))
        }
        Ok(None) => {
            println!("⚠️ Transaction not found, still pending");
            (
                StatusCode::OK,
                AxumJson(json!({
                    "success": false,
                    "failed": false,
                    "status": "pending",
                    "checkout_request_id": request.checkout_request_id,
                    "message": "Transaction still processing"
                })),
            )
        }
        Err(e) => {
            println!("❌ Database error: {}", e);
            (
                StatusCode::OK,
                AxumJson(json!({
                    "success": false,
                    "failed": false,
                    "status": "pending",
                    "checkout_request_id": request.checkout_request_id,
                    "message": "Error checking status, will retry"
                })),
            )
        }
    }
}

// ✅ HANDLER 4: Check Transaction Status (GET with query params)
pub async fn check_transaction_status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
) -> (StatusCode, AxumJson<serde_json::Value>) {
    if query.checkout_request_id.is_none() && query.merchant_request_id.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            AxumJson(json!({
                "success": false,
                "error": "checkout_request_id or merchant_request_id required"
            })),
        );
    }

    let collection: Collection<Transaction> = state.db.collection("transactions");
    let mut filter = doc! {};

    if let Some(ref id) = query.checkout_request_id {
        filter.insert("checkout_request_id", id);
    }
    if let Some(ref id) = query.merchant_request_id {
        filter.insert("merchant_request_id", id);
    }

    match collection.find_one(filter).await {
        Ok(Some(t)) => (
            StatusCode::OK,
            AxumJson(json!({
                "success": t.status == "completed",
                "status": t.status,
                "result_code": t.result_code,
                "result_desc": t.result_desc,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            AxumJson(json!({
                "success": false,
                "error": "Transaction not found"
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            AxumJson(json!({
                "success": false,
                "error": format!("{}", e)
            })),
        ),
    }
}

// ✅ HANDLER 5: Get All Transactions
pub async fn get_transactions(State(state): State<AppState>) -> AxumJson<serde_json::Value> {
    let collection: Collection<Transaction> = state.db.collection("transactions");
    match collection.find(doc! {}).await {
        Ok(mut cursor) => {
            let mut transactions = Vec::new();
            while let Some(Ok(t)) = cursor.next().await {
                transactions.push(t);
            }
            let count = transactions.len();
            AxumJson(json!({
                "success": true,
                "transactions": transactions,
                "count": count
            }))
        }
        Err(e) => AxumJson(json!({
            "success": false,
            "error": format!("{}", e)
        })),
    }
}

// ✅ HANDLER 6: Get Stats
pub async fn get_stats(State(state): State<AppState>) -> AxumJson<serde_json::Value> {
    let collection: Collection<Transaction> = state.db.collection("transactions");
    let mut total = 0;
    let mut successful = 0;
    let mut failed = 0;

    if let Ok(mut cursor) = collection.find(doc! {}).await {
        while let Some(Ok(t)) = cursor.next().await {
            total += 1;
            match t.status.as_str() {
                "completed" => successful += 1,
                "failed" => failed += 1,
                _ => {}
            }
        }
    }

    AxumJson(json!({
        "success": true,
        "total": total,
        "successful": successful,
        "failed": failed,
        "pending": total - successful - failed
    }))
}

// ✅ HANDLER 7: Simulate Payment
pub async fn simulate_payment(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> AxumJson<serde_json::Value> {
    let phone = payload
        .get("phone_number")
        .and_then(|v| v.as_str())
        .unwrap_or("254700000000");
    let amount = payload
        .get("amount")
        .and_then(|v| v.as_str())
        .unwrap_or("10");
    let user_id = payload
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("test_user");

    let transaction = Transaction {
        id: None,
        user_id: user_id.to_string(),
        phone_number: phone.to_string(),
        amount: amount.parse().unwrap_or(10.0),
        merchant_request_id: format!("SIM-{}", Utc::now().timestamp()),
        checkout_request_id: format!("ws_CO_SIM_{}", Utc::now().timestamp()),
        response_code: "0".to_string(),
        response_description: "Success".to_string(),
        customer_message: "Success".to_string(),
        status: "completed".to_string(),
        result_code: Some(0),
        result_desc: Some("Processed successfully".to_string()),
        created_at: Utc::now().to_rfc3339(),         // ✅ String
        updated_at: Utc::now().to_rfc3339(),         // ✅ String
        completed_at: Some(Utc::now().to_rfc3339()), // ✅ String
    };

    let collection: Collection<Transaction> = state.db.collection("transactions");
    let checkout_id = transaction.checkout_request_id.clone();
    let _ = collection.insert_one(&transaction).await;

    AxumJson(json!({
        "success": true,
        "checkout_request_id": checkout_id,
        "status": "completed"
    }))
}
