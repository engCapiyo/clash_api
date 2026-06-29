// File: src/routes/actions.rs

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::{
    handlers::actions::{
        cast_vote_handler,
        check_user_vote_handler,
        create_bet_handler,
        fill_bet_handler,
        get_channel_bettors_handler,
        get_channel_members_handler,
        get_channel_pledges_handler,
        get_channel_voters_handler,
        get_fixture_voters_handler,
        get_open_bets_handler,
        get_user_bets_handler,
        get_user_votes_handler,
        rollback_vote_handler, // ✅ NEW
        settle_bets_handler,
    },
    AppState,
};

pub fn actions_routes() -> Router<AppState> {
    Router::new()
        // ============================================================
        // VOTES (Global - No channel_id)
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
        .route("/vote/rollback", post(rollback_vote_handler)) // ✅ NEW
        // ============================================================
        // BETS (Channel-Specific)
        // ============================================================
        .route("/bet/create", post(create_bet_handler))
        .route("/bet/fill", post(fill_bet_handler))
        .route("/bet/settle", post(settle_bets_handler))
        .route(
            "/bet/open/:channel_id/:fixture_id",
            get(get_open_bets_handler),
        )
        .route(
            "/bet/channel/:channel_id/:fixture_id",
            get(get_channel_bettors_handler),
        )
        .route("/bet/user/:user_id", get(get_user_bets_handler))
        // ============================================================
        // CHANNELS (Filtered by membership)
        // ============================================================
        .route(
            "/channel/:channel_id/members",
            get(get_channel_members_handler),
        )
        .route(
            "/channel/:channel_id/:fixture_id/voters",
            get(get_channel_voters_handler),
        )
        .route(
            "/channel/:channel_id/:fixture_id/pledges",
            get(get_channel_pledges_handler),
        )
}
