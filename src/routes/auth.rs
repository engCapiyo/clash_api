use axum::{
    routing::{get, post, put},
    Router,
};

use crate::handlers::auth::{
    get_all_users, get_user_by_id, get_user_by_username, login, register, update_user_phone,
    update_user_points,
};
use crate::AppState;

pub fn user_routes() -> Router<AppState> {
    Router::new()
        // Auth routes
        .route("/register", post(register))
        .route("/login", post(login))
        // User retrieval routes
        .route("/users", get(get_all_users))
        .route("/user/id/:user_id", get(get_user_by_id))
        .route("/user/username/:username", get(get_user_by_username))
        // User update routes
        .route("/user/:user_id/points", put(update_user_points))
        .route("/user/:user_id/phone", put(update_user_phone))
}
