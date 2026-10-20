// File: src/routes/sub_fixtures.rs

use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    handlers::sub_fixture_handler::{
        create_sub_fixture_bet_handler, create_sub_fixture_market_handler,
        fill_sub_fixture_bet_handler, get_market_details_handler,
        get_market_sub_fixture_bets_handler, get_markets_for_match_handler,
        get_open_sub_fixture_bets_handler, get_sub_fixture_visibility_handler,
        get_user_sub_fixture_bets_handler, settle_sub_fixture_market_handler,
    },
    AppState,
};

pub fn sub_fixture_routes() -> Router<AppState> {
    Router::new()
        // ============================================================
        // MARKETS (sub-fixture definitions for a match)
        // ============================================================
        // Create (or idempotently fetch) a market.
        //   { match_id, market_type, options, line?, lock_at? }
        .route(
            "/sub-fixture/market/create",
            post(create_sub_fixture_market_handler),
        )
        // All visible markets for a match (modal "sub fixtures" list).
        .route(
            "/sub-fixture/markets/:match_id",
            get(get_markets_for_match_handler),
        )
        // Whether any visible market exists (swipe-card "has sub-fixtures" flag).
        .route(
            "/sub-fixture/visibility/:match_id",
            get(get_sub_fixture_visibility_handler),
        )
        // One market's detail + stats + every bet.
        //   :market_id accepts ObjectId or business key.
        .route(
            "/sub-fixture/market/:match_id/:market_id",
            get(get_market_details_handler),
        )
        // ============================================================
        // MONEY BETS ON A MARKET
        // ============================================================
        // Create an open money bet.
        //   { match_id, market_id, starter_id, starter_name, selection, amount }
        .route(
            "/sub-fixture/bet/create",
            post(create_sub_fixture_bet_handler),
        )
        // Fill an open money bet (stake MUST equal starter's).
        //   { match_id, market_id, finisher_id, finisher_name, selection, amount }
        .route(
            "/sub-fixture/bet/:bet_id/fill",
            post(fill_sub_fixture_bet_handler),
        )
        // Open money bets for a whole match (all markets).
        .route(
            "/sub-fixture/bets/:match_id",
            get(get_open_sub_fixture_bets_handler),
        )
        // Open + matched money bets for a single market.
        //   :market_id accepts ObjectId or business key.
        .route(
            "/sub-fixture/bets/:match_id/:market_id",
            get(get_market_sub_fixture_bets_handler),
        )
        // All money bets (any status) belonging to a user.
        .route(
            "/sub-fixture/bets/user/:user_id",
            get(get_user_sub_fixture_bets_handler),
        )
        // ============================================================
        // SETTLEMENT
        // ============================================================
        // Settle a market's money bets AND its points pokes.
        //   { match_id, market_id, winning_team: Option<String> }
        //   winning_team = None => refund everything (no winner / draw).
        .route(
            "/sub-fixture/settle",
            post(settle_sub_fixture_market_handler),
        )
}