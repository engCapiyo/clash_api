// File: src/routes/actions.rs

use axum::{
    routing::{get, post},
    Router,
};

use crate::{
    handlers::actions::{
        cast_vote_handler, check_user_vote_handler, create_bet_handler, create_poke_handler,
        fill_bet_handler, fill_poke_handler, get_channel_bettors_handler,
        get_channel_members_handler, get_channel_pledges_handler, get_channel_pokes_handler,
        get_channel_vote_count_handler, get_fixture_voters_handler, get_user_bets_handler,
        get_user_points_handler, get_user_pokes_handler, get_user_votes_handler,
        get_vote_breakdown_handler, rollback_vote_handler, settle_bets_handler,
        settle_pokes_handler,
    },
    AppState,
};

pub fn actions_routes() -> Router<AppState> {
    Router::new()
        // ============================================================
        // VOTES (Global storage, filtered by channel membership)
        // ============================================================
        .route("/vote/cast", post(cast_vote_handler))
        .route(
            "/vote/check/:fixture_id/:user_id",
            get(check_user_vote_handler),
        )
        .route(
            "/vote/fixture/:fixture_id/voters",
            get(get_fixture_voters_handler),
        )
        .route("/vote/user/:user_id", get(get_user_votes_handler))
        .route("/vote/rollback", post(rollback_vote_handler))
        .route(
            "/vote/breakdown/:fixture_id",
            get(get_vote_breakdown_handler),
        )
        // ============================================================
        // BETS (money)
        // ============================================================
        .route("/bet/create", post(create_bet_handler))
        .route("/bet/fill", post(fill_bet_handler))
        .route("/bet/settle", post(settle_bets_handler))
        .route("/bet/user/:user_id", get(get_user_bets_handler))
        // ============================================================
        // POKES (points) — main match AND sub-fixtures
        // ============================================================
        .route("/poke/create", post(create_poke_handler))
        .route("/poke/fill", post(fill_poke_handler))
        .route("/poke/settle", post(settle_pokes_handler))
        .route("/poke/user/:user_id", get(get_user_pokes_handler))
        .route("/poke/points/:user_id", get(get_user_points_handler))
        // ============================================================
        // CHANNEL FILTERED VIEWS
        // ============================================================
        .route(
            "/channel/:channel_id/:fixture_id/voters",
            get(get_channel_vote_count_handler),
        )
        .route(
            "/channel/:channel_id/:fixture_id/pledges",
            get(get_channel_pledges_handler),
        )
        .route(
            "/channel/:channel_id/:fixture_id/bettors",
            get(get_channel_bettors_handler),
        )
        // Pokes: { open: [...], matched: [...] }  (?sub_fixture_id=... for sub-fixtures)
        .route(
            "/channel/:channel_id/:fixture_id/pokes",
            get(get_channel_pokes_handler),
        )
        .route(
            "/channel/:channel_id/members",
            get(get_channel_members_handler),
        )
}