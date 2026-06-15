use axum::{
    routing::{get, post, put},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler, approve_join_request_handler, cast_vote_handler,
    check_user_vote_handler, check_user_vote_in_channel_handler, create_channel_handler,
    finalize_fixture_result_handler, get_channel_fixtures_handler, get_channel_handler,
    get_channel_invite_code_handler, get_channel_leaderboard_handler,
    get_fixture_comment_count_handler, get_fixture_latest_comment_handler, get_messages_handler,
    get_pending_requests_handler, get_single_fixture_handler, get_user_channel_count_handler,
    get_user_channel_votes_handler, get_user_channels_handler, get_user_unread_count_handler,
    get_user_votes_handler, get_weekly_top_channel_handler, initialize_fixture_chat_handler,
    join_channel_by_code_handler, leave_channel_handler, mark_chat_as_read_handler,
    reject_join_request_handler, request_join_channel_handler, reset_weekly_messages_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // ====================================================================
        // CHANNEL CRUD
        // ====================================================================
        .route("/", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        .route(
            "/:channel_id/invite-code",
            get(get_channel_invite_code_handler),
        )
        // ====================================================================
        // USER CHANNELS
        // ====================================================================
        .route("/user/:user_id", get(get_user_channels_handler))
        .route("/user/:user_id/count", get(get_user_channel_count_handler))
        // ====================================================================
        // CHANNEL MEMBERS
        // ====================================================================
        .route("/members/add", post(add_members_to_channel_handler))
        .route("/members/leave", post(leave_channel_handler))
        // ====================================================================
        // CHANNEL FIXTURES
        // ====================================================================
        .route("/:channel_id/fixtures", get(get_channel_fixtures_handler))
        .route("/fixture/chat", post(initialize_fixture_chat_handler))
        // ====================================================================
        // MESSAGES
        // ====================================================================
        .route("/messages", get(get_messages_handler))
        // ====================================================================
        // JOIN REQUESTS
        // ====================================================================
        .route("/request-join", post(request_join_channel_handler))
        .route(
            "/:channel_id/pending-requests",
            get(get_pending_requests_handler),
        )
        .route("/approve-request", post(approve_join_request_handler))
        .route("/reject-request", post(reject_join_request_handler))
        .route("/join-by-code", post(join_channel_by_code_handler))
        // ====================================================================
        // VOTES (GLOBAL - no channel_id in path)
        // ====================================================================
        .route("/votes/cast", post(cast_vote_handler))
        .route("/votes/user/:user_id", get(get_user_votes_handler))
        .route(
            "/votes/check/:fixture_id/:user_id",
            get(check_user_vote_handler),
        )
        // ====================================================================
        // CHANNEL VOTES (deprecated - kept for backward compatibility)
        // ====================================================================
        .route(
            "/:channel_id/user/:user_id/votes",
            get(get_user_channel_votes_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/user/:user_id/vote",
            get(check_user_vote_in_channel_handler),
        )
        // ====================================================================
        // FIXTURE FINALIZATION
        // ====================================================================
        .route("/fixtures/finalize", post(finalize_fixture_result_handler))
        // ====================================================================
        // LEADERBOARD & STATS
        // ====================================================================
        .route(
            "/:channel_id/leaderboard",
            get(get_channel_leaderboard_handler),
        )
        .route("/weekly/top", get(get_weekly_top_channel_handler))
        .route("/admin/reset-weekly", post(reset_weekly_messages_handler))
        // ====================================================================
        // COMMENTS & UNREAD
        // ====================================================================
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/count",
            get(get_fixture_comment_count_handler),
        )
        // GET single fixture data (❌ MISSING)
        .route(
            "/:channel_id/fixtures/:fixture_id",
            get(get_single_fixture_handler),
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
