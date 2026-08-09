use axum::{routing::post, Router};
use crate::handlers::notification_handler::{register_token, send_notification};
use crate::state::AppState;

pub fn notification_routes() -> Router<AppState> {
    Router::new()
        .route("/register-token", post(register_token))
        .route("/send", post(send_notification))
}