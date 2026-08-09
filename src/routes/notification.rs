use axum::{
    routing::{get, post, put, delete},
    Router,
};
use crate::handlers::notification_handler::{
    register_token,
    send_notification,
    send_bulk_notifications,
    get_user_notifications,
    mark_notifications_read,
    get_notification_preferences,
    update_notification_preferences,
    cleanup_expired_tokens,
    get_user_token_status,
};
use crate::state::AppState;

pub fn notification_routes() -> Router<AppState> {
    Router::new()
        .route("/register-token", post(register_token))
        .route("/send", post(send_notification))
        .route("/send-bulk", post(send_bulk_notifications))
        .route("/user/{user_id}", get(get_user_notifications))
        .route("/mark-read", put(mark_notifications_read))
        .route("/preferences/{user_id}", get(get_notification_preferences))
        .route("/preferences", put(update_notification_preferences))
        .route("/cleanup", delete(cleanup_expired_tokens))
        .route("/debug/{user_id}", get(get_user_token_status))
}