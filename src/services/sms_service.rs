use reqwest::Client;
use crate::errors::{AppError, Result};

#[derive(Clone)]
pub struct SMSService {
    api_key: String,
    username: String,
    from: String,
    client: Client,
}

impl SMSService {
    pub fn new(api_key: String, username: String, from: String) -> Self {
        Self {
            api_key,
            username,
            from,
            client: Client::new(),
        }
    }

    pub async fn send_otp(&self, phone: &str, otp: &str) -> Result<()> {
        let message = format!(
            "Your FanClash password reset OTP is: {}. Valid for 5 minutes.",
            otp
        );

        // Africa's Talking API (example - replace with your provider)
        let url = "https://api.africastalking.com/version1/messaging";

        let response = self.client
            .post(url)
            .header("apiKey", &self.api_key)
            .header("Accept", "application/json")
            .form(&[
                ("username", self.username.as_str()),
                ("to", phone),
                ("message", message.as_str()),
                ("from", self.from.as_str()),
            ])
            .send()
            .await
            .map_err(|e| AppError::ExternalApi(format!("SMS API error: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::ExternalApi(format!(
                "SMS sending failed with status: {}",
                response.status()
            )))
        }
    }
}
