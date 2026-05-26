use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct ForgotPasswordRequest {
    #[validate(length(min = 3, message = "Username must be at least 3 characters"))]
    pub username: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct VerifyOTPRequest {
    pub user_id: String,

    #[validate(length(min = 6, max = 6, message = "OTP must be 6 digits"))]
    pub otp: String,

    pub reset_token: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordRequest {
    pub user_id: String,
    pub reset_token: String,

    #[validate(length(min = 6, message = "Password must be at least 6 characters"))]
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct ForgotPasswordResponse {
    pub success: bool,
    pub message: String,
    pub user_id: Option<String>,
    pub reset_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerifyOTPResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ResetPasswordResponse {
    pub success: bool,
    pub message: String,
}
