use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub name: String,
    pub created_by: String,
    pub created_at: DateTime,
    pub members: Vec<ChannelMember>,
    pub activity: ChannelActivity,
    pub season: String,
    pub member_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelMember {
    pub user_id: String,
    pub username: String,
    pub role: String, // "admin" or "member"
    pub joined_at: DateTime,
    pub season_points: i32,
    pub correct_votes: i32,
    pub total_votes: i32,
    pub msg_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelActivity {
    pub total_messages: i32,
    pub messages_this_week: i32,
    pub week_reset_at: DateTime,
    pub last_message_at: Option<DateTime>,
}
