use crate::{handlers::bets::*, state::AppState};
use axum::{
    routing::{delete, get, post, put},
    Router,
};

pub fn bets_routes() -> Router<AppState> {
    Router::new()
        // GET /api/bets - Get all bets with optional filtering
        // POST /api/bets - Create a new bet (accept a pledge)
        .route("/", get(get_bets).post(create_bet))
        // GET /api/bets/stats - Get bet statistics
        .route("/stats", get(get_bet_stats))
        // GET /api/bets/recent - Get recent bets
        .route("/recent", get(get_recent_bets))
        // GET /api/bets/user - Get user's betting history
        .route("/user", get(get_user_bets))
        // GET /api/bets/:id - Get a single bet by ID
        // PUT /api/bets/:id - Update bet status (match result)
        // DELETE /api/bets/:id - Delete a bet
        .route(
            "/:id",
            get(get_bet_by_id).put(update_bet_status).delete(delete_bet),
        )
        // PUT /api/bets/user/balance - Update user balance
        .route("/user/balance", put(update_user_balance))
        // PUT /api/bets/:id/status - Update pledge status
        .route("/:id/status", put(update_pledge_status))
}
