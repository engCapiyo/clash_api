pub mod game;
//pub mod post;
pub mod user;

pub(crate) mod archive;
pub(crate) mod chat; // Now just a simple declaration // Now just a simple declaration
pub(crate) mod events;
mod livegames;
pub(crate) mod notification;
pub(crate) mod otp;
mod payment;
pub(crate) mod pledges;
pub(crate) mod posta;
pub mod sub_fixture; // Add this
pub(crate) mod transaction;
pub(crate) mod user_profile;
pub use sub_fixture::*;

pub mod actions;
pub(crate) mod channel;
pub(crate) mod comrade;
pub(crate) mod statistics;
pub(crate) mod comments_model;
pub(crate) mod vote;
pub(crate) mod votes_visibility; // Now just a simple declaration // Now just a simple declaration // Now just a simple declaration // Now just a simple declaration
