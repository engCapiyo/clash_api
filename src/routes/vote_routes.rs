use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::state::AppState;

pub fn vote_routes() -> Router<AppState> {
    Router::new()
        // ========== 🆕 CHAT MEDIA UPLOAD ROUTE ==========
        .route(
            "/upload/chat",
            post(crate::handlers::vote_handlers::upload_chat_media),
        )
        // ========== VOTE ROUTES ==========
        .route("/vote", post(crate::handlers::vote_handlers::create_vote))
        .route("/votes", get(crate::handlers::vote_handlers::get_votes))
        .route(
            "/votes/bulk",
            post(crate::handlers::vote_handlers::bulk_create_votes),
        )
        .route(
            "/votes/user/:voter_id",
            get(crate::handlers::vote_handlers::get_user_votes),
        )
        .route(
            "/votes/fixture/:fixture_id",
            get(crate::handlers::vote_handlers::get_fixture_votes),
        )
        .route(
            "/votes/fixture/:fixture_id/total",
            get(crate::handlers::vote_handlers::get_total_votes_for_fixture),
        )
        .route(
            "/votes/fixture/:fixture_id/selection",
            get(crate::handlers::vote_handlers::get_vote_counts_by_selection),
        )
        .route(
            "/votes/fixture/:fixture_id/user/:voter_id",
            get(crate::handlers::vote_handlers::get_user_vote_for_fixture),
        )
        .route(
            "/votes/:vote_id",
            delete(crate::handlers::vote_handlers::delete_vote),
        )
        // ========== COMMENT READ RECEIPTS ROUTE ==========
        .route(
            "/comments/seen",
            post(crate::handlers::vote_handlers::mark_comments_seen),
        )
        .route(
            "/user/:user_id/unread-counts",
            get(crate::handlers::vote_handlers::get_user_unread_counts),
        )
        // ========== LIKE ROUTES ==========
        .route("/like", post(crate::handlers::vote_handlers::create_like))
        .route(
            "/likes/fixture/:fixture_id",
            get(crate::handlers::vote_handlers::get_fixture_likes),
        )
        .route(
            "/likes/fixture/:fixture_id/total",
            get(crate::handlers::vote_handlers::get_total_likes_for_fixture),
        )
        .route(
            "/likes/fixture/:fixture_id/user/:voter_id",
            get(crate::handlers::vote_handlers::get_user_like_for_fixture),
        )
        .route(
            "/likes/:like_id",
            delete(crate::handlers::vote_handlers::delete_like),
        )
        // ========== COMMENT ROUTES ==========
        .route(
            "/comment",
            post(crate::handlers::vote_handlers::create_comment),
        )
        .route(
            "/ws/comments",
            get(crate::handlers::ws_handler::ws_comments_handler),
        )
        .route(
            "/comments",
            get(crate::handlers::vote_handlers::get_comments),
        )
        .route(
            "/comments/fixture/:fixture_id",
            get(crate::handlers::vote_handlers::get_fixture_comments),
        )
        .route(
            "/comments/fixture/:fixture_id/total",
            get(crate::handlers::vote_handlers::get_total_comments_for_fixture),
        )
        .route(
            "/comments/user/:voter_id",
            get(crate::handlers::vote_handlers::get_user_comments),
        )
        .route(
            "/comments/:comment_id",
            delete(crate::handlers::vote_handlers::delete_comment),
        )
        .route(
            "/comments/:comment_id/like",
            post(crate::handlers::vote_handlers::like_comment),
        )
    // ========== SUB-FIXTURE (PROP BETS) ROUTES ==========
}

pub fn ws_routes() -> Router<AppState> {
    Router::new().route(
        "/comments",
        get(crate::handlers::ws_handler::ws_comments_handler),
    )
}

pub fn vote_stats_routes() -> Router<AppState> {
    Router::new()
        // ========== STATISTICS ROUTES ==========
        .route(
            "/stats/votes/:fixture_id",
            get(crate::handlers::vote_handlers::get_vote_stats),
        )
        .route(
            "/stats/likes/:fixture_id",
            get(crate::handlers::vote_handlers::get_like_stats),
        )
        .route(
            "/stats/comments/:fixture_id",
            get(crate::handlers::vote_handlers::get_comment_stats),
        )
        .route(
            "/stats/fixture/:fixture_id",
            get(crate::handlers::vote_handlers::get_fixture_stats),
        )
        .route(
            "/stats/fixture/:fixture_id/all",
            get(crate::handlers::vote_handlers::get_all_counts_for_fixture),
        )
        .route(
            "/stats/fixture/:fixture_id/engagement",
            get(crate::handlers::vote_handlers::get_fixture_engagement_summary),
        )
        .route(
            "/stats/user/:voter_id",
            get(crate::handlers::vote_handlers::get_user_stats),
        )
        .route(
            "/stats/total",
            get(crate::handlers::vote_handlers::get_total_counts),
        )
        .route(
            "/stats/batch",
            post(crate::handlers::vote_handlers::get_batch_fixture_counts),
        )
        .route(
            "/stats/comments/bulk",
            post(crate::handlers::vote_handlers::get_comment_counts_for_multiple_fixtures),
        )
        .route(
            "/stats/likes/bulk",
            post(crate::handlers::vote_handlers::get_total_likes_for_multiple_fixtures),
        )
        .route(
            "/stats/combined/bulk",
            post(crate::handlers::vote_handlers::get_combined_stats_for_multiple_fixtures),
        )
        // ========== SUB-FIXTURE STATS ROUTES ==========
        // ========== REAL-TIME & TRENDING ROUTES ==========
        .route(
            "/realtime/:fixture_id",
            get(crate::handlers::vote_handlers::get_realtime_vote_updates),
        )
}

pub fn vote_admin_routes() -> Router<AppState> {
    Router::new()
        // ========== ADMIN ROUTES ==========
        .route(
            "/admin/votes/cleanup",
            post(crate::handlers::vote_handlers::cleanup_old_votes),
        )
        .route(
            "/admin/stats/overview",
            get(crate::handlers::vote_handlers::get_overview_stats),
        )
    // ========== SUB-FIXTURE ADMIN ROUTES ==========
}

// ========== FCM NOTIFICATION ROUTES ==========
pub fn notification_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/register-token",
            post(crate::handlers::notification_handler::register_token),
        )
        .route(
            "/send",
            post(crate::handlers::notification_handler::send_notification),
        )
        .route(
            "/send-bulk",
            post(crate::handlers::notification_handler::send_bulk_notifications),
        )
        .route(
            "/user/:user_id",
            get(crate::handlers::notification_handler::get_user_notifications),
        )
        .route(
            "/mark-read",
            post(crate::handlers::notification_handler::mark_notifications_read),
        )
        .route(
            "/preferences/:user_id",
            get(crate::handlers::notification_handler::get_notification_preferences),
        )
        .route(
            "/preferences",
            post(crate::handlers::notification_handler::update_notification_preferences),
        )
        .route(
            "/cleanup",
            post(crate::handlers::notification_handler::cleanup_expired_tokens),
        )
}
