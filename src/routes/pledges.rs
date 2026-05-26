use axum::{
    routing::get,
    Router,
};

use crate::state::AppState;
use crate::handlers::pledges::{
    get_pledges, create_pledge, get_pledge_stats, get_user_pledges, get_recent_pledges
};

pub fn routes() -> Router<AppState> {
    Router::new()
        // GET /api/pledges - Get all pledges with optional filtering
        // POST /api/pledges - Create a new pledge
        .route("/", get(get_pledges).post(create_pledge))

        // GET /api/pledges/stats - Get pledge statistics for a specific match
        // Example: /api/pledges/stats?home_team=Manchester%20United&away_team=Liverpool
        .route("/stats", get(get_pledge_stats))

        // GET /api/pledges/user - Get user's pledging history
        // Example: /api/pledges/user?username=john_doe
        .route("/user", get(get_user_pledges))

        // GET /api/pledges/recent - Get recent pledges (for social proof)
        .route("/recent", get(get_recent_pledges))
}