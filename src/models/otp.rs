use mongodb::bson::DateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResetOTP {
    pub code: String,        // 6-digit OTP
    pub token: String,       // Reset token
    pub attempts: i32,       // Failed attempts
    pub expires_at: DateTime, // When OTP expires
    pub created_at: DateTime, // When OTP was created
}
