use axum::{
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde_json::json;

use crate::handlers::b2c_handlers;
use crate::handlers::mpesa_handlers;
use crate::state::AppState;

pub fn mpesa_routes() -> Router<AppState> {
    Router::new()
        // Health
        .route("/health", get(mpesa_health))
        // C2B Routes
        .route("/stk-push", post(mpesa_handlers::initiate_stk_push))
        .route("/validation", post(mpesa_handlers::mpesa_validation))
        .route("/confirmation", post(mpesa_handlers::mpesa_confirmation))
        // Keep old callback for backward compatibility
        .route("/callback", post(mpesa_handlers::mpesa_confirmation))
        // ✅ NEW: Payment status check endpoint (POST for frontend)
        .route(
            "/check-payment-status",
            post(mpesa_handlers::check_payment_status),
        )
        // B2C Routes
        .route("/b2c/send", post(b2c_handlers::send_b2c_payment))
        .route("/b2c/result", post(b2c_handlers::b2c_result_callback))
        .route("/b2c/timeout", post(b2c_handlers::b2c_timeout_callback))
        // Status (GET with query params)
        .route("/status", get(mpesa_handlers::check_transaction_status))
        .route("/transactions", get(mpesa_handlers::get_transactions))
        .route("/stats", get(mpesa_handlers::get_stats))
        .route("/simulate", post(mpesa_handlers::simulate_payment))
}

async fn mpesa_health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "mpesa",
        "timestamp": Utc::now().to_rfc3339(),
        "features": ["c2b", "b2c", "stk-push", "transactions", "payment-status-check"]
    }))
}
