use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::{actions, channel, events_handler, games, statistics_handler};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // ── Games / Fixtures ────────────────────────────────────────────────────
        .route("/", get(games::get_games))
        .route("/upcoming", get(games::get_upcoming_games))
        .route("/live", get(games::get_live_games))
        .route("/history", get(games::get_history_games))
        .route("/cleanup-stale", post(games::cleanup_stale_completed_games))
        .route("/live-update", post(games::receive_live_update))
        .route("/commentary", post(games::add_commentary))
        .route(
            "/test-notification",
            post(games::send_test_notification_from_poller),
        )
        // ── Lineups (now in games) ────────────────────────────────────────────
        .route("/lineups", post(games::store_lineups))
        // ── Statistics (now in games) ─────────────────────────────────────────
        .route("/statistics", post(games::add_statistics_snapshot))
        .route("/statistics/bulk", post(games::bulk_update_statistics))
        // ── Events ─────────────────────────────────────────────────────────────
        .route("/events", post(events_handler::add_event))
        .route("/events/bulk", post(events_handler::bulk_add_events))
        // ── Batch / Fast Counts ───────────────────────────────────────────────
        .route(
            "/fixture/counts/batch",
            post(games::get_batch_fixture_counts_fast),
        )
        // ── Match-specific routes ────────────────────────────────────────────
        .route("/match/:match_id", get(games::get_game_by_match_id))
        .route(
            "/history/:match_id",
            get(games::get_history_game_by_match_id),
        )
        .route(
            "/fixture/:fixture_id/votes/fast",
            get(games::get_fixture_vote_count_fast),
        )
        .route(
            "/fixture/:fixture_id/comments/fast",
            get(games::get_fixture_comment_count_fast),
        )
        .route(
            "/fixture/:fixture_id/counts/fast",
            get(games::get_fixture_counts_fast),
        )
        .route(
            "/fixture/:fixture_id/voters",
            get(games::get_fixture_voters_fast),
        )
        .route(
            "/fixture/:fixture_id/user/:user_id/voted",
            get(games::check_user_voted_fast),
        )
        // ── Lineups (match-specific) ──────────────────────────────────────────
        .route("/:match_id/lineups", get(games::get_lineups))
        .route(
            "/:match_id/lineups/simplified",
            get(games::get_simplified_lineups),
        )
        .route(
            "/:match_id/lineups/available",
            get(games::check_lineups_available),
        )
        // ── Statistics (match-specific) ──────────────────────────────────────
        .route("/:match_id/statistics", get(games::get_match_statistics))
        .route(
            "/:match_id/statistics/latest",
            get(games::get_latest_statistics),
        )
        .route(
            "/:match_id/statistics/:minute",
            get(games::get_statistics_at_minute),
        )
        // ── :match_id routes (must come last) ─────────────────────────────────
        .route("/:match_id/score", put(games::update_game_score))
        .route("/:match_id/status", put(games::update_game_status))
        .route(
            "/:match_id/move-to-history",
            post(games::move_completed_to_history),
        )
        .route("/:match_id/commentary", get(games::get_latest_commentary))
        .route(
            "/:match_id/commentary/latest",
            get(games::get_latest_commentary),
        )
        .route("/:match_id/events", get(events_handler::get_match_events))
        .route(
            "/:match_id/events/:event_type",
            get(events_handler::get_events_by_type),
        )
        .route(
            "/:match_id/events/latest",
            get(events_handler::get_latest_event),
        )
        .route(
            "/:match_id/events",
            delete(events_handler::delete_match_events),
        )
        .route(
            "/events/:event_id",
            delete(events_handler::delete_event_by_id),
        )
        .route("/:id", get(games::get_game_by_id))
}
