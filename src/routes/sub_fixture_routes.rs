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
        // ========== SUB-FIXTURE SETTLEMENT ROUTE (NEW) ==========
        // Previously settle_sub_fixture_bets_for_market existed as a plain
        // function with no route calling it -- unreachable over HTTP. This
        // wires it up so the Python poller can settle first_goal/first_card/
        // first_corner/over_under markets as soon as it detects them.
        // ========== SUB-FIXTURE SETTLEMENT ROUTE ==========
        // Previously settle_sub_fixture_bets_for_market existed as a plain
        // function with no route calling it -- unreachable over HTTP. This
        // wires it up so the Python poller can settle first_goal/first_card/
        // first_corner/over_under markets as soon as it detects them.
        .route(
            "/sub-fixture/settle",
            post(crate::handlers::sub_fixture_handler::settle_sub_fixture_market_handler),
        )
        // ========== SUB-FIXTURE MARKET ROUTES (NEW) ==========
        // Paths match exactly what the Flutter client calls (no "/sub-fixture"
        // prefix on these two -- SwipeableVotePledgeModal hits
        // "$_api/sub_fixtures/markets/:match_id" and
        // "$_api/sub_fixtures/visibility/:match_id" directly). Both 404'd
        // before this, which is why the whole Sub-Fixtures tab silently
        // failed to load.
        .route(
            "/markets/:match_id",
            get(crate::handlers::sub_fixture_handler::get_markets_for_match_handler),
        )
        .route(
            "/visibility/:match_id",
            get(crate::handlers::sub_fixture_handler::get_sub_fixture_visibility_handler),
        )
        .route(
            "/markets/:match_id/:market_id",
            get(crate::handlers::sub_fixture_handler::get_market_details_handler),
        )
}
