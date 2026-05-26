use axum::{
    routing::{get, post, put},
    Router,
};

use crate::handlers::user_profile::{
    get_user_profiles, get_user_profile_by_id, get_user_profile_by_phone,
    save_user_profile, update_user_balance, get_user_stats,
    get_recent_users, create_user_profile
};
use crate::state::AppState;

pub fn user_profile_routes() -> Router<AppState> {
    Router::new()
        // GET routes
        .route("/profiles", get(get_user_profiles))
        .route("/profile/:id", get(get_user_profile_by_id))
        .route("/profile/phone/:phone", get(get_user_profile_by_phone))
        .route("/profile/stats", get(get_user_stats))
        .route("/profile/recent", get(get_recent_users))

        // POST routes
       // .route("/save-profile", post(save_user_profile))
        .route("/create_profile", post(create_user_profile))
        .route("/update-balance", post(update_user_balance))

        // PUT routes (for updates)
        .route("/profiles/:id", put(save_user_profile))
}