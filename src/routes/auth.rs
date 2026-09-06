use axum::{
    routing::{get, post, put,delete},
    Router,
};

use crate::handlers::auth::{
    check_user_exists, get_all_users, get_user_by_id, get_user_by_username, login, pin_login,
    register, set_pin, update_user_phone, update_user_points, delete_user, delete_users_bulk, delete_users_by_username,
};
use crate::AppState;

pub fn user_routes() -> Router<AppState> {
    Router::new()
        // Auth routes - Firebase OTP
        .route("/register", post(register))
        .route("/login", post(login))
        // Auth routes - PIN fallback
        .route("/pin-login", post(pin_login))
        .route("/check-user/:phone", get(check_user_exists))
        .route("/set-pin/:user_id", post(set_pin))
        // User retrieval routes
        .route("/users", get(get_all_users))
        .route("/user/id/:user_id", get(get_user_by_id))
        .route("/user/username/:username", get(get_user_by_username))
        // User update routes
        .route("/user/:user_id/points", put(update_user_points))
        .route("/user/:user_id/phone", put(update_user_phone))
          .route("/user/:user_id", delete(delete_user))
        .route("/users/bulk-delete", delete(delete_users_bulk))
        .route("/users/by-username", delete(delete_users_by_username))
}
