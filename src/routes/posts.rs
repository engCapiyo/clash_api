use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::posta;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Post routes - root level
        .route("/", get(posta::get_posts))
        .route(
            "/",
            post(create_post).layer(DefaultBodyLimit::max(60 * 1024 * 1024)),
        )
        .route("/search", get(posta::search_posts))
        .route("/stats", get(posta::get_post_stats))
        // Post-specific routes
        .route("/:post_id", get(posta::get_post_by_id))
        .route("/:post_id", put(posta::update_post_caption))
        .route("/:post_id", delete(posta::delete_post))
        // Video routes
        .route("/:post_id/video", get(posta::get_post_video))
        .route(
            "/:post_id/video/thumbnail",
            get(posta::get_post_video_thumbnail),
        )
        // Like routes
        .route("/:post_id/like", post(posta::like_post))
        .route("/:post_id/unlike", post(posta::unlike_post))
        // Comment routes
        .route("/:post_id/comments", get(posta::get_comments))
        .route("/:post_id/comments", post(posta::create_comment))
        // User routes
        .route("/user/:user_id", get(posta::get_posts_by_user))
        .route("/user/:user_id/all", delete(posta::delete_posts_by_user))
        .route("/stats/user/:user_id", get(posta::get_user_post_stats))
}

pub fn comment_routes() -> Router<AppState> {
    Router::new()
        .route("/:comment_id", put(posta::update_comment))
        .route("/:comment_id", delete(posta::delete_comment))
        .route("/:comment_id/like", post(posta::like_comment))
        .route("/:comment_id/unlike", post(posta::unlike_comment))
}

pub fn upload_routes() -> Router<AppState> {
    Router::new().route("/upload", post(posta::create_post))
}
