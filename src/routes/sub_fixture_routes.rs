use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

pub fn sub_fixture_routes() -> Router<AppState> {
    Router::new()
        // ========== SUB-FIXTURE BET ROUTES ==========
        .route(
            "/sub-fixture/bet",
            post(crate::handlers::sub_fixture_handler::create_sub_fixture_bet_handler),
        )
        .route(
            "/sub-fixture/bet/:bet_id/fill",
            post(crate::handlers::sub_fixture_handler::fill_sub_fixture_bet_handler),
        )
        .route(
            "/sub-fixture/bets/open/:match_id",
            get(crate::handlers::sub_fixture_handler::get_open_sub_fixture_bets_handler),
        )
        .route(
            "/sub-fixture/bets/user/:user_id",
            get(crate::handlers::sub_fixture_handler::get_user_sub_fixture_bets_handler),
        )
        .route(
            "/sub-fixture/bets/market/:match_id/:market_id",
            get(crate::handlers::sub_fixture_handler::get_market_sub_fixture_bets_handler),
        )
    // ========== SUB-FIXTURE MARKET ROUTES ==========
}
