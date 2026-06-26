use axum::{routing::get, Router};

use crate::handlers::pledges::{
    create_pledge, get_pledge_stats, get_pledges, get_recent_pledges, get_user_pledges,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_pledges).post(create_pledge))
        .route("/stats", get(get_pledge_stats))
        .route("/user", get(get_user_pledges))
        .route("/recent", get(get_recent_pledges))
}
