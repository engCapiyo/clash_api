use axum::{
    routing::{get, post, delete},
    Router,
};

use crate::{
    handlers::archive,
    state::AppState,
};

pub fn archive_routes() -> Router<AppState> {
    Router::new()
        // Create new archive activity (when user votes/likes/comments)
        .route("/activity", post(archive::create_archive_activity))

        // Get user's archive activities
        .route("/user/:user_id", get(archive::get_user_archive))
        .route("/getAllVotes", get(archive::get_all_archives))

        // Get activities for specific fixture
        .route("/fixture/:fixture_id", get(archive::get_fixture_archive))

        // Get user's archive stats
        .route("/stats/:user_id", get(archive::get_user_archive_stats))

        // Search/filter archive activities
        .route("/search", get(archive::search_archive_activities))

        // Delete archive activity (optional)
        .route("/activity/:id", delete(archive::delete_archive_activity))
}
