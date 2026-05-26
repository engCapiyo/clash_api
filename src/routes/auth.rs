use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        // Registration
        .route("/register", post(crate::handlers::auth::register))
        // Get users
        .route("/users", get(crate::handlers::auth::get_all_users))
        // Get user by phone (login)
        .route(
            "/user/phone/:phone",
            get(crate::handlers::auth::get_user_by_phone),
        )
        // Get user by username (check availability)
        .route(
            "/user/username/:username",
            get(crate::handlers::auth::get_user_by_username),
        )
}
