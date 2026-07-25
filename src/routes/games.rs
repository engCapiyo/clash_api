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
        // ── Friendly-fixtures resolution (seed / midnight-surrender) ──────────
        .route("/seed", post(games::seed_fixture))
        .route("/pending-resolution", get(games::get_pending_resolution))
        // ── Lineups ─────────────────────────────────────────────────────────────
        .route("/lineups", post(games::store_lineups))
        // ── Statistics ──────────────────────────────────────────────────────────
        .route("/statistics", post(games::add_statistics_snapshot))
        .route("/statistics/bulk", post(games::bulk_update_statistics))
        // ── Events ──────────────────────────────────────────────────────────────
        .route("/events", post(events_handler::add_event))
        .route("/events/bulk", post(events_handler::bulk_add_events))
        // ── Match-specific routes ──────────────────────────────────────────────
        .route("/match/:match_id", get(games::get_game_by_match_id))
        .route(
            "/history/:match_id",
            get(games::get_history_game_by_match_id),
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
        // ── Commentary ─────────────────────────────────────────────────────────
        .route("/:match_id/commentary", get(games::get_latest_commentary))
        .route(
            "/:match_id/commentary/latest",
            get(games::get_latest_commentary),
        )
        .route("/commentary/bulk", post(games::add_commentary_bulk))
        // ── Events (match-specific) ──────────────────────────────────────────
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
        // ── Score & Status ─────────────────────────────────────────────────────
        .route("/:match_id/score", put(games::update_game_score))
        .route("/:match_id/status", put(games::update_game_status))
        .route(
            "/:match_id/move-to-history",
            post(games::move_completed_to_history),
        )
        // ── Friendly-fixtures resolve / abandon ────────────────────────────────
        .route("/:match_id/resolve", post(games::resolve_fixture))
        .route("/:match_id/abandon", post(games::abandon_fixture))
        // ── Get by ID (must come last) ──────────────────────────────────────
        .route("/:id", get(games::get_game_by_id))
}
