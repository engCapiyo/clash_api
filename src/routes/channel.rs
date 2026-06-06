use axum::{
    routing::{get, post},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler, cast_vote_handler, check_user_vote_in_channel_handler,
    create_channel_handler, finalize_fixture_result_handler, get_channel_fixtures_handler,
    get_channel_handler, get_channel_leaderboard_handler, get_fixture_comment_count_handler,
    get_fixture_latest_comment_handler, get_messages_handler, get_user_channel_count_handler,
    get_user_channel_votes_handler, get_user_channels_handler, get_weekly_top_channel_handler,
    initialize_fixture_chat_handler, leave_channel_handler, reset_weekly_messages_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // Existing routes
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
        .route("/", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        .route(
            "/:channel_id/leaderboard",
            get(get_channel_leaderboard_handler),
        )
        .route("/:channel_id/fixtures", get(get_channel_fixtures_handler))
        .route(
            "/:channel_id/user/:user_id/votes",
            get(get_user_channel_votes_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/user/:user_id/vote",
            get(check_user_vote_in_channel_handler),
        )
        // NEW: Comment count endpoint
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/count",
            get(get_fixture_comment_count_handler),
        )
        // NEW: Latest comment endpoint
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/latest",
            get(get_fixture_latest_comment_handler),
        )
}
