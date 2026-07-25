use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler, approve_join_request_handler, cast_vote_handler,
    check_user_liked_handler, check_user_vote_handler, check_user_vote_in_channel_handler,
    compute_admin_payout_handler, compute_admin_reward_score_handler,
    compute_all_admin_payouts_handler, compute_all_admin_reward_scores_handler,
    create_channel_handler, create_pledge_with_vote_handler, delete_message_handler,
    finalize_fixture_result_handler, get_admin_reward_leaderboard_handler,
    get_all_channels_handler, get_channel_fixtures_handler, get_channel_handler,
    get_channel_invite_code_handler, get_channel_leaderboard_handler, get_channel_votes_handler,
    get_fixture_comment_count_handler, get_fixture_latest_comment_handler,
    get_fixture_likes_handler, get_fixture_pledgers_handler, get_invite_channel_handler,
    get_message_thread_handler, get_messages_handler, get_pending_requests_handler,
    get_single_fixture_handler, get_single_message_handler, get_user_channel_count_handler,
    get_user_channel_votes_handler, get_user_channels_handler, get_user_liked_fixtures_handler,
    get_user_unread_count_handler, get_user_votes_handler, get_weekly_top_channel_handler,
    initialize_fixture_chat_handler, join_channel_by_code_handler, leave_channel_handler,
    mark_chat_as_read_handler, reject_join_request_handler, request_join_channel_handler,
    reset_weekly_messages_handler, send_message_handler, toggle_like_handler,
    upload_chat_media_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

// ============================================================================
// WEBSOCKET ROUTES
// ============================================================================

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

// ============================================================================
// MAIN CHANNEL ROUTES
// ============================================================================

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // ====================================================================
        // CHANNEL CRUD
        // ====================================================================
        .route("/create", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        .route("/all", get(get_all_channels_handler))
        .route("/invite/:code", get(get_invite_channel_handler))
        .route(
            "/:channel_id/admin-payout/compute",
            post(compute_admin_payout_handler).get(compute_admin_payout_handler),
        )
        .route(
            "/admin-payout/compute-all",
            post(compute_all_admin_payouts_handler).get(compute_all_admin_payouts_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/votes",
            get(get_channel_votes_handler),
        )
        .route(
            "/admin-reward/compute-all",
            post(compute_all_admin_reward_scores_handler)
                .get(compute_all_admin_reward_scores_handler),
        )
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
        .route(
            "/:channel_id/fixtures/:fixture_id",
            get(get_single_fixture_handler),
        )
        // ====================================================================
        // ✅ CHAT MEDIA UPLOAD - WITH 60MB LIMIT
        // ====================================================================
        .route(
            "/media/upload",
            post(upload_chat_media_handler).layer(DefaultBodyLimit::max(60 * 1024 * 1024)),
        )
        // ====================================================================
        // MESSAGES - FULL CRUD WITH CAPTION & MEDIA SUPPORT
        // ====================================================================
        .route("/:channel_id/messages", get(get_messages_handler))
        .route("/:channel_id/messages", post(send_message_handler))
        .route(
            "/:channel_id/messages/:message_id",
            get(get_single_message_handler),
        )
        .route(
            "/:channel_id/messages/:message_id",
            delete(delete_message_handler),
        )
        .route(
            "/:channel_id/messages/:message_id/thread",
            get(get_message_thread_handler),
        )
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
        // PLEDGES
        // ====================================================================
        .route("/pledges", post(create_pledge_with_vote_handler))
        .route(
            "/:channel_id/fixtures/:fixture_id/pledgers",
            get(get_fixture_pledgers_handler),
        )
        // ====================================================================
        // VOTES
        // ====================================================================
        .route("/votes/cast", post(cast_vote_handler))
        .route("/votes/user/:user_id", get(get_user_votes_handler))
        .route(
            "/votes/check/:fixture_id/:user_id",
            get(check_user_vote_handler),
        )
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
        // ADMIN REWARD SCORING
        // ====================================================================
        .route(
            "/:channel_id/admin-reward/compute",
            post(compute_admin_reward_score_handler),
        )
        .route(
            "/admin-reward/leaderboard",
            get(get_admin_reward_leaderboard_handler),
        )
        // ====================================================================
        // COMMENTS & UNREAD
        // ====================================================================
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
        // ====================================================================
        // LIKES
        // ====================================================================
        .route("/likes/toggle", post(toggle_like_handler))
        .route(
            "/:channel_id/fixtures/:fixture_id/likes",
            get(get_fixture_likes_handler),
        )
        .route(
            "/likes/check/:user_id/:channel_id/:fixture_id",
            get(check_user_liked_handler),
        )
        .route(
            "/likes/user/:user_id/channel/:channel_id",
            get(get_user_liked_fixtures_handler),
        )
}

// ============================================================================
// COMBINED ROUTES (Optional - if you want to combine everything)
// ============================================================================

pub fn combined_routes() -> Router<AppState> {
    Router::new()
        .nest("/api/ws", ws_channel_routes())
        .nest("/api/channels", channel_routes())
}
