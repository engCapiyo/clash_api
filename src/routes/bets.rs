use axum::{
    Router,
    routing::{get, post, put, delete},
};
use crate::{state::AppState, handlers::bets::*};

pub fn bets_routes() -> Router<AppState> {
    Router::new()
        .route("/create_bets", get(get_bets).post(create_bet))
        .route("/stats", get(get_bet_stats))
        .route("/recent", get(get_recent_bets))
        .route("/get_userbets", get(get_user_bets))
        .route("/bets/:id", get(get_bet_by_id).put(update_bet_status).delete(delete_bet))
        .route("/user/balance", put(update_user_balance))
        .route("/:id/status", put(update_pledge_status))
}