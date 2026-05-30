use axum::{
    routing::{get, post},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler, cast_vote_handler, create_channel_handler,
    finalize_fixture_result_handler, get_channel_fixtures_handler, get_channel_handler,
    get_channel_leaderboard_handler, get_messages_handler, get_user_channel_count_handler,
    get_user_channels_handler, get_weekly_top_channel_handler, initialize_fixture_chat_handler,
    leave_channel_handler, reset_weekly_messages_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // =============================================================
        // SPECIFIC ROUTES — no dynamic segments, MUST come FIRST
        // =============================================================
        .route("/user/:user_id/count", get(get_user_channel_count_handler))
        .route("/weekly/top", get(get_weekly_top_channel_handler))
        .route("/fixture/chat", post(initialize_fixture_chat_handler))
        .route("/members/add", post(add_members_to_channel_handler))
        .route("/members/leave", post(leave_channel_handler))
        .route("/messages", get(get_messages_handler))
        .route("/user/:user_id", get(get_user_channels_handler))
        .route("/votes", post(cast_vote_handler))
        .route("/admin/reset-weekly", post(reset_weekly_messages_handler))
        .route("/fixtures/finalize", post(finalize_fixture_result_handler))
        // =============================================================
        // DYNAMIC ROUTES — with path parameters, MUST come LAST
        // =============================================================
        .route("/", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        .route(
            "/:channel_id/leaderboard",
            get(get_channel_leaderboard_handler),
        )
        .route("/:channel_id/fixtures", get(get_channel_fixtures_handler))
}
