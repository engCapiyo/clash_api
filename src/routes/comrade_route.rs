use crate::handlers::comrade_handler::{
    add_comrade,
    get_available_users,
    get_comrade_stats,
    get_user_comrades,
    remove_comrade,
    upgrade_comrade_limit, // Add this if you have it
};
use crate::state::AppState;
use axum::{
    routing::{delete, get, post},
    Router,
};

pub fn comrade_routes() -> Router<AppState> {
    Router::new()
        // Add/Remove comrade
        .route("/comrades/add", post(add_comrade))
        .route("/comrades/remove", post(remove_comrade))
        // Get comrades
        .route("/comrades/:user_id", get(get_user_comrades))
        .route("/comrades/:user_id/stats", get(get_comrade_stats))
        // Get available users for selection
        .route("/comrades/available/:user_id", get(get_available_users))
        // Upgrade limit (paid tier)
        .route("/comrades/upgrade/:user_id", post(upgrade_comrade_limit))
}
