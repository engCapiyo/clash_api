pub(crate) mod archive;
//pub(crate) mod auth;
pub mod auth;
pub(crate) mod bets;
pub(crate) mod chat;
pub(crate) mod comrade_route;
pub(crate) mod games;
pub(crate) mod mpesa;
pub(crate) mod pledges;
pub(crate) mod posts;
pub(crate) mod user_profile;
pub(crate) mod vote_routes;
// pub mod auth;  // Remove or comment out if not needed

// Re-export all public functions
pub use vote_routes::*;
