use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::events_handler;
use crate::handlers::games;
use crate::handlers::lineup_handler;
use crate::handlers::statistics_handler;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // GAME ROUTES
        .route("/", get(games::get_games))
        .route("/upcoming", get(games::get_upcoming_games))
        .route("/live", get(games::get_live_games))
        .route("/:id", get(games::get_game_by_id))
        .route("/match/:match_id", get(games::get_game_by_match_id))
        .route("/:match_id/score", put(games::update_game_score))
        .route("/:match_id/status", put(games::update_game_status))
        .route("/lineups", post(games::store_lineups))
        .route("/:match_id/lineups", get(games::get_lineups))
        .route("/history", get(games::get_history_games))
        .route(
            "/history/:match_id",
            get(games::get_history_game_by_match_id),
        )
        .route(
            "/:match_id/move-to-history",
            post(games::move_completed_to_history),
        )
        .route("/cleanup-stale", post(games::cleanup_stale_completed_games))
        // FAST COUNT ENDPOINTS
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
            "/fixture/counts/batch",
            post(games::get_batch_fixture_counts_fast),
        )
        // VOTER ENDPOINTS
        .route(
            "/fixture/:fixture_id/voters",
            get(games::get_fixture_voters_fast),
        )
        .route(
            "/fixture/:fixture_id/user/:user_id/voted",
            get(games::check_user_voted_fast),
        )
        // EVENTS ENDPOINTS
        .route("/:match_id/events", get(events_handler::get_match_events))
        .route(
            "/:match_id/events/:event_type",
            get(events_handler::get_events_by_type),
        )
        .route(
            "/:match_id/events/latest",
            get(events_handler::get_latest_event),
        )
        .route("/events", post(events_handler::add_event))
        .route("/events/bulk", post(events_handler::bulk_add_events))
        .route(
            "/:match_id/events",
            delete(events_handler::delete_match_events),
        )
        .route(
            "/events/:event_id",
            delete(events_handler::delete_event_by_id),
        )
        // ========== COMMENTARY ENDPOINTS (NEW) ==========
        .route("/commentary", post(games::add_commentary))
        .route("/:match_id/commentary", get(games::get_match_commentary))
        .route(
            "/:match_id/commentary/latest",
            get(games::get_latest_commentary),
        )
        // STATISTICS ENDPOINTS
        .route(
            "/statistics",
            post(statistics_handler::add_statistics_snapshot),
        )
        .route(
            "/statistics/bulk",
            post(statistics_handler::bulk_update_statistics),
        )
        .route(
            "/:match_id/statistics",
            get(statistics_handler::get_match_statistics),
        )
        .route(
            "/:match_id/statistics/latest",
            get(statistics_handler::get_latest_statistics),
        )
        .route(
            "/:match_id/statistics/:minute",
            get(statistics_handler::get_statistics_at_minute),
        )
        // LINEUPS ENDPOINTS
        .route("/lineups", post(lineup_handler::receive_lineups_update))
        .route("/:match_id/lineups", get(lineup_handler::get_lineups))
        .route(
            "/:match_id/lineups/simplified",
            get(lineup_handler::get_simplified_lineups),
        )
        .route(
            "/:match_id/lineups/available",
            get(lineup_handler::check_lineups_available),
        )
        // LIVE UPDATE ENDPOINT
        .route("/live-update", post(games::receive_live_update))
        // TEST NOTIFICATION ENDPOINT
        .route(
            "/test-notification",
            post(games::send_test_notification_from_poller),
        )
}
