use std::env;

/// Central app configuration — loaded once at startup from environment variables.
/// Every field has a safe default so the server never panics if optional
/// services (M-Pesa, SMS, Cloudinary) are not yet configured.
#[derive(Debug, Clone)]
pub struct AppConfig {
    // ── Core ─────────────────────────────────────────────────────────────────
    pub jwt_secret: String,

    // ── SMS (Africa's Talking) ────────────────────────────────────────────────
    pub sms_api_key: String,
    pub sms_username: String,
    pub sms_from: String,

    // ── M-Pesa ───────────────────────────────────────────────────────────────
    pub mpesa_environment: String, // "sandbox" | "production"
    pub mpesa_consumer_key: String,
    pub mpesa_consumer_secret: String,
    pub mpesa_short_code: String,
    pub mpesa_passkey: String,
    pub mpesa_initiator_name: String,
    pub mpesa_security_credential: String,

    // Callback URLs (C2B)
    pub mpesa_confirmation_url: String,
    pub mpesa_validation_url: String,

    // Callback URLs (B2C)
    pub mpesa_b2c_result_url: String,
    pub mpesa_b2c_queue_timeout_url: String,
}

impl AppConfig {
    /// Load all config from environment variables.
    /// Missing optional vars emit a warning instead of panicking.
    pub fn from_env() -> Self {
        // ── Core ──────────────────────────────────────────────────────────────
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| {
            tracing::error!("❌ JWT_SECRET not set — tokens will be invalid");
            "insecure-default-change-me-in-production".to_string()
        });

        // ── SMS ───────────────────────────────────────────────────────────────
        let sms_api_key = env::var("SMS_API_KEY").unwrap_or_else(|_| {
            tracing::warn!("⚠️  SMS_API_KEY not set — OTP SMS will not send");
            String::new()
        });
        let sms_username = env::var("SMS_USERNAME").unwrap_or_else(|_| "sandbox".to_string());
        let sms_from = env::var("SMS_FROM").unwrap_or_else(|_| "FanClash".to_string());

        // ── M-Pesa ────────────────────────────────────────────────────────────
        let mpesa_environment =
            env::var("MPESA_ENVIRONMENT").unwrap_or_else(|_| "sandbox".to_string());

        let mpesa_consumer_key = env::var("MPESA_CONSUMER_KEY").unwrap_or_else(|_| {
            tracing::warn!("⚠️  MPESA_CONSUMER_KEY not set");
            String::new()
        });
        let mpesa_consumer_secret = env::var("MPESA_CONSUMER_SECRET").unwrap_or_else(|_| {
            tracing::warn!("⚠️  MPESA_CONSUMER_SECRET not set");
            String::new()
        });

        // Daraja sandbox default shortcode
        let mpesa_short_code =
            env::var("MPESA_SHORT_CODE").unwrap_or_else(|_| "174379".to_string());

        let mpesa_passkey = env::var("MPESA_PASSKEY").unwrap_or_else(|_| {
            tracing::warn!("⚠️  MPESA_PASSKEY not set — STK push will fail");
            String::new()
        });
        let mpesa_initiator_name =
            env::var("MPESA_INITIATOR_NAME").unwrap_or_else(|_| "testapi".to_string());
        let mpesa_security_credential =
            env::var("MPESA_SECURITY_CREDENTIAL").unwrap_or_else(|_| {
                tracing::warn!("⚠️  MPESA_SECURITY_CREDENTIAL not set — B2C will fail");
                String::new()
            });

        // Derive callback URLs from API_BASE_URL if not set explicitly
        let base_url = env::var("API_BASE_URL")
            .unwrap_or_else(|_| "https://fanclash-api.onrender.com".to_string());

        let mpesa_confirmation_url = env::var("MPESA_CONFIRMATION_URL")
            .unwrap_or_else(|_| format!("{}/api/mpesa/confirmation", base_url));
        let mpesa_validation_url = env::var("MPESA_VALIDATION_URL")
            .unwrap_or_else(|_| format!("{}/api/mpesa/validation", base_url));
        let mpesa_b2c_result_url = env::var("MPESA_B2C_RESULT_URL")
            .unwrap_or_else(|_| format!("{}/api/mpesa/b2c/result", base_url));
        let mpesa_b2c_queue_timeout_url = env::var("MPESA_B2C_QUEUE_TIMEOUT_URL")
            .unwrap_or_else(|_| format!("{}/api/mpesa/b2c/timeout", base_url));

        if !mpesa_consumer_key.is_empty() {
            tracing::info!(
                "✅ M-Pesa configured — environment: {}, shortcode: {}",
                mpesa_environment,
                mpesa_short_code
            );
        }

        AppConfig {
            jwt_secret,
            sms_api_key,
            sms_username,
            sms_from,
            mpesa_environment,
            mpesa_consumer_key,
            mpesa_consumer_secret,
            mpesa_short_code,
            mpesa_passkey,
            mpesa_initiator_name,
            mpesa_security_credential,
            mpesa_confirmation_url,
            mpesa_validation_url,
            mpesa_b2c_result_url,
            mpesa_b2c_queue_timeout_url,
        }
    }

    /// True when running against the live M-Pesa production API.
    pub fn is_production(&self) -> bool {
        self.mpesa_environment.to_lowercase() == "production"
    }

    /// True when M-Pesa credentials are actually set.
    pub fn is_mpesa_configured(&self) -> bool {
        !self.mpesa_consumer_key.is_empty()
            && !self.mpesa_consumer_secret.is_empty()
            && !self.mpesa_passkey.is_empty()
    }

    /// True when SMS credentials are set.
    pub fn is_sms_configured(&self) -> bool {
        !self.sms_api_key.is_empty()
    }

    /// Returns the three M-Pesa API URLs for (auth, STK push, B2C).
    /// Automatically switches between sandbox and production.
    pub fn get_mpesa_urls(&self) -> (String, String, String) {
        if self.is_production() {
            (
                // Production auth
                "https://api.safaricom.co.ke/oauth/v1/generate?grant_type=client_credentials"
                    .to_string(),
                // Production STK push
                "https://api.safaricom.co.ke/mpesa/stkpush/v1/processrequest".to_string(),
                // Production B2C
                "https://api.safaricom.co.ke/mpesa/b2c/v1/paymentrequest".to_string(),
            )
        } else {
            (
                // Sandbox auth
                "https://sandbox.safaricom.co.ke/oauth/v1/generate?grant_type=client_credentials"
                    .to_string(),
                // Sandbox STK push
                "https://sandbox.safaricom.co.ke/mpesa/stkpush/v1/processrequest".to_string(),
                // Sandbox B2C
                "https://sandbox.safaricom.co.ke/mpesa/b2c/v1/paymentrequest".to_string(),
            )
        }
    }
}
