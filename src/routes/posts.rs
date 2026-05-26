
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
        .route("/", post(posta::create_post))
        .route("/search", get(posta::search_posts))
        .route("/stats", get(posta::get_post_stats))

        // Post-specific routes - using post_id explicitly
        .route("/:post_id", get(posta::get_post_by_id))
        .route("/:post_id", put(posta::update_post_caption))
        .route("/:post_id", delete(posta::delete_post))
        .route("/:post_id/thumbnail/:width/:height", get(posta::get_post_thumbnail))
        .route("/:post_id/transform/:transformations", get(posta::get_post_with_transform))

        // Like routes for posts
        .route("/:post_id/like", post(posta::like_post))
        .route("/:post_id/unlike", post(posta::unlike_post))
       // .route("/:comment_id/unlike", post(posta::unlike_comment))

        // Comment routes (nested under posts)
        .route("/:post_id/comments", get(posta::get_comments))
        .route("/:post_id/comments", post(posta::create_comment))

        // User-related routes
        .route("/user/:user_id", get(posta::get_posts_by_user))
        .route("/user/:user_id/all", delete(posta::delete_posts_by_user))
        .route("/stats/user/:user_id", get(posta::get_user_post_stats))
}

pub fn comment_routes() -> Router<AppState> {
    Router::new()
        // Comment-specific routes in their own router
        .route("/:comment_id", put(posta::update_comment))
        .route("/:comment_id", delete(posta::delete_comment))
        .route("/:comment_id/like", post(posta::like_comment))
        .route("/:comment_id/unlike", post(posta::unlike_comment))
}

pub fn upload_routes() -> Router<AppState> {
    Router::new().route("/upload", post(posta::create_post))
}
