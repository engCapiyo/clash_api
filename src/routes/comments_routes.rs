// File: src/routes/comments.rs

use axum::{
    routing::{get, post, put, delete},
    Router,
};



use crate::{
    handlers::comments_handlers::{
        create_comment,
        get_comments,
        get_comment_by_id,
        update_comment,
        delete_comment,
        like_comment,
        unlike_comment,
        get_comment_count,
        get_comment_replies,
        get_user_comments,
    },
    AppState,
};

pub fn comments_routes() -> Router<AppState> {
    Router::new()
        // ============================================================
        // POST COMMENTS (Top-level comments on posts)
        // ============================================================
        .route(
            "/posts/:post_id/comments",
            get(get_comments).post(create_comment),
        )
        // ============================================================
        // COMMENT BY ID (CRUD operations)
        // ============================================================
        .route(
            "/comments/:comment_id",
            get(get_comment_by_id)
                .put(update_comment)
                .delete(delete_comment),
        )
        // ============================================================
        // COMMENT LIKES
        // ============================================================
        .route(
            "/comments/:comment_id/like",
            post(like_comment).delete(unlike_comment),
        )
        // ============================================================
        // COMMENT COUNT FOR POST
        // ============================================================
        .route(
            "/posts/:post_id/comments/count",
            get(get_comment_count),
        )
        // ============================================================
        // COMMENT REPLIES (Nested replies to a specific comment)
        // ============================================================
        .route(
            "/comments/:comment_id/replies",
            get(get_comment_replies),
        )
        // ============================================================
        // USER COMMENTS (Get all comments by a specific user)
        // ============================================================
        .route(
            "/users/:user_id/comments",
            get(get_user_comments),
        )
}