use axum::{
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde_json::json;

use crate::handlers::mpesa_handlers;
use crate::state::AppState;

pub fn mpesa_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(mpesa_health))
        .route("/stk-push", post(mpesa_handlers::initiate_stk_push))
        .route("/validation", post(mpesa_handlers::mpesa_validation))
        .route("/confirmation", post(mpesa_handlers::mpesa_confirmation))
        .route("/callback", post(mpesa_handlers::mpesa_confirmation))
        .route("/check-payment-status", post(mpesa_handlers::check_payment_status))
        .route("/status", get(mpesa_handlers::check_transaction_status))
        .route("/transactions", get(mpesa_handlers::get_transactions))
        .route("/stats", get(mpesa_handlers::get_stats))
        .route("/simulate", post(mpesa_handlers::simulate_payment))
}

async fn mpesa_health() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "ok",
        "service": "mpesa",
        "timestamp": Utc::now().to_rfc3339(),
        "features": ["c2b", "b2c", "stk-push", "transactions", "payment-status-check"]
    }))
}