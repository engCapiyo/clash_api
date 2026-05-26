// src/routes/chat_routes.rs
use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::handlers::chat_handlers;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // Get messages for a specific post
        .route("/chat/:post_id/messages", get(chat_handlers::get_post_messages))
        // In your router setup (probably in main.rs or a routes file)
       // .route("/chat/users/:user_id/messages", get(chat_handlers::get_user_messages))
        // Create a new message in a post
        .route("/chat/:post_id/messages", post(chat_handlers::create_message))
        // Get a specific message by ID
        .route("/chat/messages/:message_id", get(chat_handlers::get_message))
        // Update a message (user's own only)
        .route("/chat/messages/:message_id", put(chat_handlers::update_message))
        // Delete a message (user's own only)
        .route("/chat/messages/:message_id", delete(chat_handlers::delete_message))
        // Mark multiple messages as seen
        .route("/chat/messages/mark-seen", post(chat_handlers::mark_messages_as_seen))
        // Get unread message count for a post
        .route("/chat/:post_id/unread-count", get(chat_handlers::get_unread_count))
        // Get messages by user ID
        .route("/chat/users/:user_id/messages", get(chat_handlers::get_user_messages))
}
