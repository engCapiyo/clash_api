use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use futures_util::StreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};
use mongodb::Collection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};

use crate::models::transaction::{MpesaTransaction, Transaction};
use crate::models::user::User;
use crate::state::AppState;

// ============================================================================
// REQUEST / RESPONSE STRUCTS
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct StkPushRequest {
    pub phone_number: String,
    pub amount: String,
    pub account_reference: Option<String>,
    pub transaction_desc: Option<String>,
    pub user_id: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct B2CPaymentRequest {
    pub phone_number: String,
    pub amount: String,
    pub remarks: String,
    pub occasion: Option<String>,
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
}

// ============================================================================
// HELPERS
// ============================================================================

fn now_str() -> String {
    Utc::now().to_rfc3339()
}

fn normalize_phone(phone: &str) -> String {
    let mut cleaned: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if cleaned.starts_with('0') {
        cleaned = cleaned[1..].to_string();
    }
    if cleaned.len() > 9 {
        cleaned = cleaned[cleaned.len() - 9..].to_string();
    }
    cleaned
}

// ============================================================================
// HANDLERS - ALL USE AppState
// ============================================================================

pub async fn mpesa_validation(Json(_payload): Json<serde_json::Value>) -> Json<serde_json::Value> {
    println!("✅ [VALIDATION] Received from Safaricom — auto-approving");
    Json(json!({ "ResultCode": 0, "ResultDesc": "Success" }))
}

pub async fn initiate_stk_push(
    State(state): State<AppState>,
    Json(request): Json<StkPushRequest>,
) -> Json<serde_json::Value> {
    println!(
        "🔵 [STK] Initiating STK push — phone: {}, amount: {}",
        request.phone_number, request.amount
    );

    if request.phone_number.is_empty() || request.amount.is_empty() {
        return Json(json!({ "success": false, "error": "Phone number and amount are required" }));
    }

    // ✅ Validate user_id
    if request.user_id.is_none() || request.user_id.as_ref().unwrap().is_empty() {
        return Json(json!({ "success": false, "error": "User ID is required" }));
    }

    let amount: f64 = match request.amount.parse() {
        Ok(a) if a > 0.0 => a,
        _ => {
            return Json(json!({ "success": false, "error": "Amount must be greater than 0" }));
        }
    };

    let mpesa_service = match &state.mpesa_service {
        Some(s) => s,
        None => {
            return Json(json!({ "success": false, "error": "M-Pesa service is not available" }));
        }
    };

    let response = match mpesa_service
        .initiate_stk_push(
            &request.phone_number,
            &request.amount,
            request.account_reference.as_deref(),
            request.transaction_desc.as_deref(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({ "success": false, "error": e.to_string() }));
        }
    };

    let now = now_str();

    // ✅ STORE user_id in transaction
    let transaction = Transaction {
        id: None,
        user_id: request.user_id.clone().unwrap(), // ✅ Store user_id
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
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
    };

    let collection: Collection<Transaction> = state.db.collection("transactions");
    if let Err(e) = collection.insert_one(&transaction).await {
        println!("⚠️ Failed to save transaction: {}", e);
    }

    println!("✅ [STK] Push initiated successfully");
    Json(json!({
        "success": true,
        "CheckoutRequestID": response.checkout_request_id,
        "checkout_request_id": response.checkout_request_id,
        "merchant_request_id": response.merchant_request_id,
        "response_code": response.response_code,
        "response_description": response.response_description,
        "customer_message": response.customer_message,
    }))
}
pub async fn mpesa_confirmation(
    State(state): State<AppState>,
    Json(payload): Json<MpesaCallback>,
) -> Json<serde_json::Value> {
    println!("🎯 [CALLBACK] Received at {}", now_str());

    let callback = payload.Body.stk_callback;
    let checkout_id = callback.checkout_request_id.clone();

    println!("🎯 CheckoutRequestID : {}", checkout_id);
    println!("🎯 ResultCode        : {}", callback.result_code);
    println!("🎯 ResultDesc        : {}", callback.result_desc);

    info!("Received M-Pesa callback for: {}", checkout_id);

    if callback.merchant_request_id.is_empty() || callback.checkout_request_id.is_empty() {
        error!("Invalid callback: missing required fields");
        return Json(json!({ "ResultCode": 1, "ResultDesc": "Invalid callback data" }));
    }

    let transactions: Collection<Transaction> = state.db.collection("transactions");
    let filter = doc! { "checkout_request_id": &checkout_id };

    match transactions.find_one(filter).await {
        Ok(Some(transaction)) => {
            println!("✅ Found transaction in database");

            let mut mpesa_receipt_number: Option<String> = None;
            let mut paid_amount: Option<f64> = None;
            let mut transaction_date: Option<String> = None;
            let mut paying_phone: Option<String> = None;

            if let Some(metadata) = &callback.callback_metadata {
                for item in &metadata.items {
                    match item.name.as_str() {
                        "Amount" => {
                            if let serde_json::Value::Number(n) = &item.value {
                                paid_amount = n.as_f64();
                                println!("💰 Amount: Ksh {}", paid_amount.unwrap_or(0.0));
                            }
                        }
                        "MpesaReceiptNumber" => {
                            if let serde_json::Value::String(r) = &item.value {
                                mpesa_receipt_number = Some(r.clone());
                                println!("🧾 Receipt: {}", r);
                            }
                        }
                        "TransactionDate" => {
                            transaction_date = Some(item.value.to_string());
                        }
                        "PhoneNumber" => {
                            if let serde_json::Value::Number(n) = &item.value {
                                paying_phone = Some(n.to_string());
                            } else if let serde_json::Value::String(s) = &item.value {
                                paying_phone = Some(s.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }

            let status = if callback.result_code == 0 {
                "completed"
            } else {
                "failed"
            };
            let now = now_str();

            let mut set_doc = doc! {
                "status": status,
                "result_code": callback.result_code,
                "result_desc": &callback.result_desc,
                "updated_at": &now,
            };

            if callback.result_code == 0 {
                set_doc.insert("completed_at", &now);
            }

            let update = doc! { "$set": set_doc };

            let id = match &transaction.id {
                Some(id) => id,
                None => {
                    error!("Transaction missing _id field!");
                    return Json(
                        json!({ "ResultCode": 1, "ResultDesc": "Transaction missing ID" }),
                    );
                }
            };

            match transactions.update_one(doc! { "_id": id }, update).await {
                Ok(r) if r.matched_count > 0 => {
                    println!("✅ Updated transaction {} → {}", checkout_id, status);
                    info!("Updated transaction {} to {}", checkout_id, status);
                }
                Ok(_) => {
                    println!("⚠️ No document matched _id");
                    error!("No document matched _id for transaction update");
                }
                Err(e) => {
                    println!("❌ DB update error: {}", e);
                    error!("Failed to update transaction: {}", e);
                }
            }

            // ✅ UPDATE USER BALANCE BY user_id (from transaction)
            if callback.result_code == 0 {
                let amount_to_add = paid_amount.unwrap_or(transaction.amount);
                let user_id = transaction.user_id.clone(); // ✅ Get user_id from transaction

                println!(
                    "💰 Adding {} to user balance for user_id: {}",
                    amount_to_add, user_id
                );

                let users: Collection<User> = state.db.collection("users");

                // ✅ Parse user_id to ObjectId
                let user_oid = match bson::oid::ObjectId::parse_str(&user_id) {
                    Ok(oid) => oid,
                    Err(e) => {
                        error!("❌ Invalid user_id format: {} - {}", user_id, e);
                        return Json(json!({ "ResultCode": 1, "ResultDesc": "Invalid user_id" }));
                    }
                };

                let now_bson = BsonDateTime::now();

                // ✅ Update balance by user_id
                let update_balance = doc! {
                    "$inc": { "balance": amount_to_add },
                    "$set": { "updated_at": now_bson }
                };

                match users
                    .update_one(doc! { "_id": user_oid }, update_balance)
                    .await
                {
                    Ok(result) => {
                        if result.matched_count > 0 {
                            println!("✅ User balance updated successfully for user: {}", user_id);
                            info!("💰 Added {} to user {} balance", amount_to_add, user_id);
                        } else {
                            println!("⚠️ User not found: {}", user_id);
                            error!("User not found: {}", user_id);
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to update user balance: {}", e);
                    }
                }

                // ✅ Save MpesaTransaction record
                if let Some(receipt) = mpesa_receipt_number {
                    let mpesa_tx = MpesaTransaction {
                        id: None,
                        paying_phone_number: paying_phone
                            .unwrap_or_else(|| transaction.phone_number.clone()),
                        transaction_date: transaction_date.unwrap_or_else(|| now.clone()),
                        mpesa_receipt_number: receipt,
                        paid_amount: amount_to_add.to_string(),
                        merchant_request_id: callback.merchant_request_id.clone(),
                        checkout_request_id: checkout_id.clone(),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    };

                    let mpesa_transactions: Collection<MpesaTransaction> =
                        state.db.collection("mpesa_transactions");

                    if let Err(e) = mpesa_transactions.insert_one(&mpesa_tx).await {
                        error!("Failed to insert MpesaTransaction record: {}", e);
                    } else {
                        println!("🎉 MpesaTransaction record saved");
                        info!("💰 Payment successful for checkout {}", checkout_id);
                    }
                }
            } else {
                println!("❌ Payment failed: {}", callback.result_desc);
            }
        }
        Ok(None) => {
            println!("⚠️ Transaction NOT FOUND for checkout_id: {}", checkout_id);
            warn!("Transaction not found for callback: {}", checkout_id);
        }
        Err(e) => {
            println!("❌ Database query error: {}", e);
            error!("Failed to find transaction: {}", e);
        }
    }

    println!("📤 Returning success to Safaricom");
    Json(json!({ "ResultCode": 0, "ResultDesc": "Success" }))
}

pub async fn check_payment_status(
    State(state): State<AppState>,
    Json(request): Json<StatusRequest>,
) -> Json<serde_json::Value> {
    println!("🔍 Checking status for: {}", request.checkout_request_id);

    let collection: Collection<Transaction> = state.db.collection("transactions");
    let filter = doc! { "checkout_request_id": &request.checkout_request_id };

    match collection.find_one(filter).await {
        Ok(Some(t)) => {
            let is_success = t.status == "completed";
            let is_failed = t.status == "failed";

            let receipt = if is_success {
                let mpesa_col: Collection<MpesaTransaction> =
                    state.db.collection("mpesa_transactions");
                mpesa_col
                    .find_one(doc! { "checkout_request_id": &request.checkout_request_id })
                    .await
                    .ok()
                    .flatten()
                    .map(|m| m.mpesa_receipt_number)
            } else {
                None
            };

            Json(json!({
                "success": is_success,
                "failed": is_failed,
                "status": t.status,
                "result_code": t.result_code,
                "result_desc": t.result_desc,
                "checkout_request_id": t.checkout_request_id,
                "merchant_request_id": t.merchant_request_id,
                "amount": t.amount,
                "phone_number": t.phone_number,
                "mpesa_receipt": receipt,
                "updated_at": t.updated_at,
                "created_at": t.created_at,
            }))
        }
        Ok(None) => Json(json!({
            "success": false,
            "failed": false,
            "status": "pending",
            "checkout_request_id": request.checkout_request_id,
            "message": "Transaction still processing"
        })),
        Err(e) => Json(json!({
            "success": false,
            "failed": false,
            "status": "pending",
            "checkout_request_id": request.checkout_request_id,
            "message": format!("Error checking status: {}", e)
        })),
    }
}

pub async fn check_transaction_status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
) -> Json<serde_json::Value> {
    if query.checkout_request_id.is_none() && query.merchant_request_id.is_none() {
        return Json(json!({
            "success": false,
            "error": "checkout_request_id or merchant_request_id required"
        }));
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
        Ok(Some(t)) => {
            let receipt = if t.status == "completed" {
                let mpesa_col: Collection<MpesaTransaction> =
                    state.db.collection("mpesa_transactions");
                let key = t.checkout_request_id.clone();
                mpesa_col
                    .find_one(doc! { "checkout_request_id": &key })
                    .await
                    .ok()
                    .flatten()
                    .map(|m| m.mpesa_receipt_number)
            } else {
                None
            };

            Json(json!({
                "success": t.status == "completed",
                "status": t.status,
                "result_code": t.result_code,
                "result_desc": t.result_desc,
                "mpesa_receipt": receipt,
            }))
        }
        Ok(None) => Json(json!({ "success": false, "error": "Transaction not found" })),
        Err(e) => Json(json!({ "success": false, "error": format!("{}", e) })),
    }
}

pub async fn get_transactions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let collection: Collection<Transaction> = state.db.collection("transactions");
    match collection.find(doc! {}).await {
        Ok(mut cursor) => {
            let mut transactions = Vec::new();
            while let Some(Ok(t)) = cursor.next().await {
                transactions.push(t);
            }
            let count = transactions.len();
            Json(json!({ "success": true, "transactions": transactions, "count": count }))
        }
        Err(e) => Json(json!({ "success": false, "error": format!("{}", e) })),
    }
}

pub async fn get_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
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

    Json(json!({
        "success": true,
        "total": total,
        "successful": successful,
        "failed": failed,
        "pending": total - successful - failed
    }))
}

pub async fn simulate_payment(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
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

    let now = now_str();
    let ts = Utc::now().timestamp();

    let transaction = Transaction {
        id: None,
        user_id: user_id.to_string(),
        phone_number: phone.to_string(),
        amount: amount.parse().unwrap_or(10.0),
        merchant_request_id: format!("SIM-{}", ts),
        checkout_request_id: format!("ws_CO_SIM_{}", ts),
        response_code: "0".to_string(),
        response_description: "Success".to_string(),
        customer_message: "Success".to_string(),
        status: "completed".to_string(),
        result_code: Some(0),
        result_desc: Some("Processed successfully".to_string()),
        created_at: now.clone(),
        updated_at: now.clone(),
        completed_at: Some(now.clone()),
    };

    let transactions: Collection<Transaction> = state.db.collection("transactions");
    let checkout_id = transaction.checkout_request_id.clone();
    let _ = transactions.insert_one(&transaction).await;

    let mpesa_tx = MpesaTransaction {
        id: None,
        paying_phone_number: phone.to_string(),
        transaction_date: now.clone(),
        mpesa_receipt_number: format!("SIM{}ABCDEF", ts),
        paid_amount: amount.to_string(),
        merchant_request_id: format!("SIM-{}", ts),
        checkout_request_id: checkout_id.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    let mpesa_col: Collection<MpesaTransaction> = state.db.collection("mpesa_transactions");
    let _ = mpesa_col.insert_one(&mpesa_tx).await;

    Json(json!({
        "success": true,
        "checkout_request_id": checkout_id,
        "status": "completed"
    }))
}

#[axum::debug_handler]
pub async fn initiate_b2c_payment(
    State(state): State<AppState>,
    Json(request): Json<B2CPaymentRequest>,
) -> Json<serde_json::Value> {
    println!(
        "🔵 [B2C] Initiating B2C payment — phone: {}, amount: {}",
        request.phone_number, request.amount
    );

    if request.phone_number.is_empty() || request.amount.is_empty() {
        return Json(json!({ "success": false, "error": "Phone number and amount are required" }));
    }

    let amount: f64 = match request.amount.parse() {
        Ok(a) if a > 0.0 => a,
        _ => {
            return Json(json!({ "success": false, "error": "Amount must be greater than 0" }));
        }
    };

    let users: Collection<User> = state.db.collection("users");
    let normalized = normalize_phone(&request.phone_number);

    let phone_filters = vec![
        doc! { "phone": &request.phone_number },
        doc! { "phone": format!("254{}", normalized) },
        doc! { "phone": format!("0{}", normalized) },
        doc! { "phone": { "$regex": format!("{}$", normalized) } },
    ];

    let mut user = None;
    for filter in phone_filters {
        if let Ok(Some(u)) = users.find_one(filter).await {
            user = Some(u);
            break;
        }
    }

    let user = match user {
        Some(u) => u,
        None => {
            return Json(json!({ "success": false, "error": "User not found" }));
        }
    };

    let current_balance = user.balance;
    if current_balance < amount {
        return Json(json!({
            "success": false,
            "error": format!("Insufficient balance: have KES {}, need KES {}", current_balance, amount)
        }));
    }

    let now_bson = BsonDateTime::now();
    let user_id = user.id.clone().unwrap();

    let update = doc! {
        "$inc": { "balance": -amount },
        "$set": { "updated_at": now_bson }
    };

    match users.update_one(doc! { "_id": user_id }, update).await {
        Ok(result) => {
            if result.matched_count == 0 {
                return Json(json!({ "success": false, "error": "Failed to deduct balance" }));
            }
            println!("✅ User balance deducted: KES {:.2}", amount);
        }
        Err(e) => {
            return Json(json!({ "success": false, "error": format!("{}", e) }));
        }
    }

    let mpesa_service = match state.mpesa_service.clone() {
        Some(s) => s,
        None => {
            let rollback = doc! {
                "$inc": { "balance": amount },
                "$set": { "updated_at": BsonDateTime::now() }
            };
            let _ = users.update_one(doc! { "_id": user_id }, rollback).await;
            return Json(json!({ "success": false, "error": "M-Pesa service unavailable" }));
        }
    };

    let response = match mpesa_service
        .send_b2c_payment(
            &request.phone_number,
            &request.amount,
            "BusinessPayment",
            &request.remarks,
            request.occasion.as_deref(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let rollback = doc! {
                "$inc": { "balance": amount },
                "$set": { "updated_at": BsonDateTime::now() }
            };
            let _ = users.update_one(doc! { "_id": user_id }, rollback).await;
            return Json(json!({ "success": false, "error": format!("{}", e) }));
        }
    };

    if response.response_code != "0" {
        let rollback = doc! {
            "$inc": { "balance": amount },
            "$set": { "updated_at": BsonDateTime::now() }
        };
        let _ = users.update_one(doc! { "_id": user_id }, rollback).await;
        return Json(json!({
            "success": false,
            "error": format!("M-Pesa declined: {}", response.response_description)
        }));
    }

    let now = now_str();
    let transaction = Transaction {
        id: None,
        user_id: user.id.clone().unwrap().to_string(),
        phone_number: request.phone_number.clone(),
        amount: -amount,
        merchant_request_id: response.originator_conversation_id.clone(),
        checkout_request_id: response.conversation_id.clone(),
        response_code: response.response_code.clone(),
        response_description: response.response_description.clone(),
        customer_message: "Withdrawal successful".to_string(),
        status: "completed".to_string(),
        result_code: Some(0),
        result_desc: Some("Payment sent".to_string()),
        created_at: now.clone(),
        updated_at: now.clone(),
        completed_at: Some(now.clone()),
    };

    let transactions: Collection<Transaction> = state.db.collection("transactions");
    let _ = transactions.insert_one(&transaction).await;

    let new_balance = current_balance - amount;

    Json(json!({
        "success": true,
        "conversation_id": response.conversation_id,
        "originator_conversation_id": response.originator_conversation_id,
        "response_code": response.response_code,
        "response_description": response.response_description,
        "amount": amount,
        "previous_balance": current_balance,
        "new_balance": new_balance,
        "user_id": user.id.unwrap().to_string(),
    }))
}
