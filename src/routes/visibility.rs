// routes.rs

use axum::{
    routing::{get, post},
    Router,
};

use crate::handlers::visibility_handler;
use crate::state::AppState;

pub fn visibility_routes() -> Router<AppState> {
    Router::new()
        .route("/:key", get(visibility_handler::get_visibility))
        .route("/:key", post(visibility_handler::update_visibility))
        .route("/batch", get(visibility_handler::get_batch_visibility))
}
