use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("MongoDB error: {0}")]
    MongoDB(#[from] mongodb::error::Error),

    #[error("Multipart error: {0}")]
    Multipart(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid image format")]
    InvalidImageFormat,

    #[error("Image too large")]
    ImageTooLarge,

    #[error("No image provided")]
    NoImageProvided,

    #[error("Invalid user data")]
    InvalidUserData,

    // ── Auth-specific errors ──────────────────────────────
    #[error("User not found")]
    UserNotFound,

    #[error("Invalid password")]
    InvalidPassword,

    #[error("User already exists")]
    UserAlreadyExists,

    #[error("Missing required field: {0}")]
    MissingRequiredField(String),

    // NEW OTP errors
    #[error("Phone not verified")]
    PhoneNotVerified,

    #[error("Invalid or expired OTP")]
    InvalidOtp,

    #[error("OTP session expired")]
    OtpExpired,
    // ─────────────────────────────────────────────────────
    #[error("Post not found")]
    PostNotFound,

    #[error("Invalid ObjectId: {0}")]
    InvalidObjectId(String),

    #[error("Document not found")]
    DocumentNotFound,

    #[error("Duplicate key error")]
    DuplicateKey,

    #[error("M-Pesa error: {0}")]
    MpesaError(String),

    #[error("Authentication error")]
    AuthError,

    #[error("Unauthorized access")]
    Unauthorized,

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("External API error: {0}")]
    ExternalApi(String),

    #[error("Cloudinary error: {0}")]
    CloudinaryError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Service error: {0}")]
    ServiceError(String),

    #[error("Redis error: {0}")]
    RedisError(String),

    #[error("HTTP client error: {0}")]
    HttpClientError(String),

    #[error("Internal server error: {0}")]
    InternalServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AppError::MongoDB(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ),
            AppError::Multipart(_) => (
                StatusCode::BAD_REQUEST,
                "Invalid multipart data".to_string(),
            ),
            AppError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IO error".to_string()),
            AppError::InvalidImageFormat => {
                (StatusCode::BAD_REQUEST, "Invalid image format".to_string())
            }
            AppError::ImageTooLarge => (StatusCode::BAD_REQUEST, "Image too large".to_string()),
            AppError::NoImageProvided => (StatusCode::BAD_REQUEST, "No image provided".to_string()),
            AppError::InvalidUserData => (StatusCode::BAD_REQUEST, "Invalid user data".to_string()),

            AppError::UserNotFound => (StatusCode::NOT_FOUND, "User not found".to_string()),
            AppError::InvalidPassword => (StatusCode::UNAUTHORIZED, "Invalid password".to_string()),
            AppError::UserAlreadyExists => {
                (StatusCode::CONFLICT, "User already exists".to_string())
            }
            AppError::MissingRequiredField(f) => (
                StatusCode::BAD_REQUEST,
                format!("Missing required field: {}", f),
            ),

            // NEW OTP error mappings
            AppError::PhoneNotVerified => (
                StatusCode::FORBIDDEN,
                "Phone number not verified".to_string(),
            ),
            AppError::InvalidOtp => (
                StatusCode::BAD_REQUEST,
                "Invalid or expired OTP".to_string(),
            ),
            AppError::OtpExpired => (
                StatusCode::BAD_REQUEST,
                "OTP has expired. Please request a new one".to_string(),
            ),

            AppError::PostNotFound => (StatusCode::NOT_FOUND, "Post not found".to_string()),
            AppError::DocumentNotFound => (StatusCode::NOT_FOUND, "Document not found".to_string()),
            AppError::InvalidObjectId(_) => {
                (StatusCode::BAD_REQUEST, "Invalid ID format".to_string())
            }
            AppError::DuplicateKey => (StatusCode::CONFLICT, "Duplicate entry".to_string()),
            AppError::MpesaError(_) => (StatusCode::BAD_GATEWAY, "M-Pesa error".to_string()),
            AppError::AuthError => (
                StatusCode::UNAUTHORIZED,
                "Authentication failed".to_string(),
            ),
            AppError::Unauthorized => (StatusCode::FORBIDDEN, "Unauthorized access".to_string()),
            AppError::ValidationError(_) => {
                (StatusCode::BAD_REQUEST, "Validation failed".to_string())
            }
            AppError::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded".to_string(),
            ),
            AppError::ServiceUnavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service unavailable".to_string(),
            ),
            AppError::ExternalApi(_) => (StatusCode::BAD_GATEWAY, "External API error".to_string()),
            AppError::CloudinaryError(_) => {
                (StatusCode::BAD_GATEWAY, "Cloudinary error".to_string())
            }
            AppError::ConfigurationError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Configuration error".to_string(),
            ),
            AppError::ServiceError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Service error".to_string(),
            ),
            AppError::RedisError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Redis error".to_string())
            }
            AppError::HttpClientError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "HTTP client error".to_string(),
            ),
            AppError::InternalServerError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
        };

        let body = Json(json!({
            "error": error_message,
            "message": self.to_string(),
            "success": false,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }));

        (status, body).into_response()
    }
}

// ── From impls ────────────────────────────────────────────────
impl From<axum_extra::extract::multipart::MultipartError> for AppError {
    fn from(err: axum_extra::extract::multipart::MultipartError) -> Self {
        AppError::Multipart(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::ValidationError(format!("JSON parsing error: {}", err))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::ExternalApi(format!("HTTP request failed: {}", err))
    }
}

impl From<mongodb::bson::oid::Error> for AppError {
    fn from(err: mongodb::bson::oid::Error) -> Self {
        AppError::InvalidObjectId(err.to_string())
    }
}

impl From<std::num::ParseFloatError> for AppError {
    fn from(err: std::num::ParseFloatError) -> Self {
        AppError::ValidationError(format!("Number parsing error: {}", err))
    }
}

impl From<std::num::ParseIntError> for AppError {
    fn from(err: std::num::ParseIntError) -> Self {
        AppError::ValidationError(format!("Integer parsing error: {}", err))
    }
}

// ── Helper constructors ───────────────────────────────────────
impl AppError {
    pub fn invalid_data(msg: impl Into<String>) -> Self {
        AppError::ValidationError(msg.into())
    }

    pub fn missing_field(field: impl Into<String>) -> Self {
        AppError::MissingRequiredField(field.into())
    }

    pub fn mpesa(msg: impl Into<String>) -> Self {
        AppError::MpesaError(msg.into())
    }

    pub fn external_api(msg: impl Into<String>) -> Self {
        AppError::ExternalApi(msg.into())
    }

    pub fn cloudinary(msg: impl Into<String>) -> Self {
        AppError::CloudinaryError(msg.into())
    }

    pub fn configuration(msg: impl Into<String>) -> Self {
        AppError::ConfigurationError(msg.into())
    }

    pub fn service(msg: impl Into<String>) -> Self {
        AppError::ServiceError(msg.into())
    }

    pub fn redis(msg: impl Into<String>) -> Self {
        AppError::RedisError(msg.into())
    }

    pub fn internal_server_error(msg: impl Into<String>) -> Self {
        AppError::InternalServerError(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
