use axum::{
    routing::{get, post, put},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler,
    cast_vote_handler,
    check_user_vote_handler, // Changed: removed _in_channel
    create_channel_handler,
    finalize_fixture_result_handler,
    get_channel_fixtures_handler,
    get_channel_handler,
    get_channel_leaderboard_handler,
    get_fixture_comment_count_handler,
    get_fixture_latest_comment_handler,
    get_messages_handler,
    get_user_channel_count_handler,
    get_user_channels_handler,
    get_user_unread_count_handler,
    get_user_votes_handler, // Changed: removed _channel
    get_weekly_top_channel_handler,
    initialize_fixture_chat_handler,
    leave_channel_handler,
    mark_chat_as_read_handler,
    reset_weekly_messages_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // Channel CRUD
        .route("/", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        // User channels
        .route("/user/:user_id", get(get_user_channels_handler))
        .route("/user/:user_id/count", get(get_user_channel_count_handler))
        // Channel members
        .route("/members/add", post(add_members_to_channel_handler))
        .route("/members/leave", post(leave_channel_handler))
        // Channel fixtures
        .route("/:channel_id/fixtures", get(get_channel_fixtures_handler))
        .route("/fixture/chat", post(initialize_fixture_chat_handler))
        // Messages
        .route("/messages", get(get_messages_handler))
        // Votes (GLOBAL - no channel_id in path)
        .route("/votes/cast", post(cast_vote_handler))
        .route("/votes/user/:user_id", get(get_user_votes_handler))
        .route(
            "/votes/check/:fixture_id/:user_id",
            get(check_user_vote_handler),
        )
        // Fixture finalization
        .route("/fixtures/finalize", post(finalize_fixture_result_handler))
        // Leaderboard & stats
        .route(
            "/:channel_id/leaderboard",
            get(get_channel_leaderboard_handler),
        )
        .route("/weekly/top", get(get_weekly_top_channel_handler))
        .route("/admin/reset-weekly", post(reset_weekly_messages_handler))
        // Comments & unread
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/count",
            get(get_fixture_comment_count_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/latest",
            get(get_fixture_latest_comment_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/unread/:user_id",
            get(get_user_unread_count_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/read/:user_id",
            put(mark_chat_as_read_handler),
        )
}
