// mpesa_service.rs
use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use chrono::Utc;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::config::AppConfig;

// C2B Structs
#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub expires_in: String,
}

#[derive(Debug, Serialize)]
pub struct StkPushRequest {
    #[serde(rename = "BusinessShortCode")]
    pub business_short_code: String,
    #[serde(rename = "Password")]
    pub password: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    #[serde(rename = "TransactionType")]
    pub transaction_type: String,
    #[serde(rename = "Amount")]
    pub amount: String,
    #[serde(rename = "PartyA")]
    pub party_a: String,
    #[serde(rename = "PartyB")]
    pub party_b: String,
    #[serde(rename = "PhoneNumber")]
    pub phone_number: String,
    #[serde(rename = "CallBackURL")]
    pub callback_url: String,
    #[serde(rename = "AccountReference")]
    pub account_reference: String,
    #[serde(rename = "TransactionDesc")]
    pub transaction_desc: String,
}

#[derive(Debug, Deserialize)]
pub struct StkPushResponse {
    #[serde(rename = "MerchantRequestID")]
    pub merchant_request_id: String,
    #[serde(rename = "CheckoutRequestID")]
    pub checkout_request_id: String,
    #[serde(rename = "ResponseCode")]
    pub response_code: String,
    #[serde(rename = "ResponseDescription")]
    pub response_description: String,
    #[serde(rename = "CustomerMessage")]
    pub customer_message: String,
}

// B2C Structs
#[derive(Debug, Serialize)]
pub struct B2CRequest {
    #[serde(rename = "InitiatorName")]
    pub initiator_name: String,
    #[serde(rename = "SecurityCredential")]
    pub security_credential: String,
    #[serde(rename = "CommandID")]
    pub command_id: String,
    #[serde(rename = "Amount")]
    pub amount: String,
    #[serde(rename = "PartyA")]
    pub party_a: String,
    #[serde(rename = "PartyB")]
    pub party_b: String,
    #[serde(rename = "Remarks")]
    pub remarks: String,
    #[serde(rename = "QueueTimeOutURL")]
    pub queue_timeout_url: String,
    #[serde(rename = "ResultURL")]
    pub result_url: String,
    #[serde(rename = "Occasion")]
    pub occasion: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct B2CResponse {
    #[serde(rename = "ConversationID")]
    pub conversation_id: String,
    #[serde(rename = "OriginatorConversationID")]
    pub originator_conversation_id: String,
    #[serde(rename = "ResponseCode")]
    pub response_code: String,
    #[serde(rename = "ResponseDescription")]
    pub response_description: String,
}

#[derive(Debug, Clone)]
pub struct MpesaService {
    config: AppConfig,
    client: Client,
    cached_token: Arc<RwLock<Option<(String, chrono::DateTime<Utc>)>>>,
}

impl MpesaService {
    pub fn new(config: AppConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        println!(
            "[MPESA SERVICE] Initialized in {} mode",
            if config.is_production() {
                "PRODUCTION"
            } else {
                "SANDBOX"
            }
        );

        MpesaService {
            config,
            client,
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    fn format_phone_number(&self, phone: &str) -> String {
        let phone = phone
            .trim()
            .replace(" ", "")
            .replace("-", "")
            .replace("+", "");

        if phone.starts_with("254") && phone.len() == 12 {
            return phone.to_string();
        }
        if phone.starts_with("07") && phone.len() == 10 {
            return format!("254{}", &phone[1..]);
        }
        if phone.starts_with("7") && phone.len() == 9 {
            return format!("254{}", phone);
        }

        // If it's already 12 digits but doesn't start with 254
        if phone.len() == 12 && phone.chars().all(char::is_numeric) {
            return phone.to_string();
        }

        warn!("Unexpected phone format: {}", phone);
        phone.to_string()
    }

    fn generate_password(&self, timestamp: &str) -> String {
        let password_string = format!(
            "{}{}{}",
            self.config.mpesa_short_code, self.config.mpesa_passkey, timestamp
        );
        base64.encode(password_string)
    }

    pub async fn get_access_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        {
            let cached = self.cached_token.read().unwrap();
            if let Some((token, expiry)) = cached.as_ref() {
                if *expiry > Utc::now() + chrono::Duration::minutes(5) {
                    info!("Using cached access token");
                    return Ok(token.clone());
                }
            }
        }

        info!("Requesting new access token");
        let auth_string = format!(
            "{}:{}",
            self.config.mpesa_consumer_key, self.config.mpesa_consumer_secret
        );
        let encoded_auth = base64.encode(auth_string);

        let (auth_url, _, _) = self.config.get_mpesa_urls();

        println!("[AUTH] URL: {}", auth_url);
        println!(
            "[AUTH] Consumer Key length: {}",
            self.config.mpesa_consumer_key.len()
        );

        let response = self
            .client
            .get(&auth_url)
            .header(header::AUTHORIZATION, format!("Basic {}", encoded_auth))
            .send()
            .await?;

        let status = response.status();
        let headers = response.headers().clone();

        if !status.is_success() {
            let body = response.text().await?;
            error!("Failed to get access token: {} - {}", status, body);
            error!("Response headers: {:?}", headers);
            return Err(format!("M-Pesa auth failed: {} - {}", status, body).into());
        }

        let auth_response: AuthResponse = response.json().await?;

        println!(
            "[AUTH] Token obtained: {}... (first 20 chars)",
            &auth_response.access_token[..20.min(auth_response.access_token.len())]
        );

        {
            let expiry_time = Utc::now() + chrono::Duration::hours(1);
            let mut cached = self.cached_token.write().unwrap();
            *cached = Some((auth_response.access_token.clone(), expiry_time));
        }

        info!("Access token obtained successfully");
        Ok(auth_response.access_token)
    }

    // C2B: Customer to Business
    pub async fn initiate_stk_push(
        &self,
        phone_number: &str,
        amount: &str,
        account_reference: Option<&str>,
        transaction_desc: Option<&str>,
    ) -> Result<StkPushResponse, Box<dyn std::error::Error>> {
        info!("C2B: STK push for {} - KSh {}", phone_number, amount);

        let amount_parsed = amount.parse::<f64>()?;
        if amount_parsed <= 0.0 {
            return Err("Amount must be greater than 0".into());
        }

        let access_token = self.get_access_token().await?;
        let formatted_phone = self.format_phone_number(phone_number);
        let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let password = self.generate_password(&timestamp);

        let (_, stk_url, _) = self.config.get_mpesa_urls();

        let stk_request = StkPushRequest {
            business_short_code: self.config.mpesa_short_code.clone(),
            password,
            timestamp,
            transaction_type: "CustomerPayBillOnline".to_string(),
            amount: amount.to_string(),
            party_a: formatted_phone.clone(),
            party_b: self.config.mpesa_short_code.clone(),
            phone_number: formatted_phone,
            callback_url: self.config.mpesa_confirmation_url.clone(),
            account_reference: account_reference.unwrap_or("FanClash").to_string(),
            transaction_desc: transaction_desc
                .unwrap_or("Payment for services")
                .to_string(),
        };

        println!("[STK] Request: {:?}", serde_json::to_string(&stk_request)?);

        let response = self
            .client
            .post(&stk_url)
            .header(header::AUTHORIZATION, format!("Bearer {}", access_token))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&stk_request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            error!("C2B failed: {} - {}", status, body);
            return Err(format!("C2B failed: {}", status).into());
        }

        let stk_response: StkPushResponse = response.json().await?;
        info!("C2B initiated: {}", stk_response.merchant_request_id);
        Ok(stk_response)
    }

    // B2C: Business to Customer
    pub async fn send_b2c_payment(
        &self,
        phone_number: &str,
        amount: &str,
        command_id: &str,
        remarks: &str,
        occasion: Option<&str>,
    ) -> Result<B2CResponse, Box<dyn std::error::Error>> {
        info!("B2C: Sending to {} - KSh {}", phone_number, amount);

        println!("==========================================");
        println!("🚀 B2C TRANSACTION INITIATED");
        println!("Environment: {}", self.config.mpesa_environment);
        println!("Phone: {}", phone_number);
        println!("Amount: KSh {}", amount);
        println!("Command ID: {}", command_id);
        println!("Remarks: {}", remarks);
        println!("Business Shortcode: {}", self.config.mpesa_short_code);
        println!("Initiator: {}", self.config.mpesa_initiator_name);
        println!(
            "Security Credential length: {}",
            self.config.mpesa_security_credential.len()
        );
        println!("==========================================");

        // CRITICAL: Check if this is production
        if self.config.is_production() {
            println!("⚠️  WARNING: This is PRODUCTION - REAL MONEY will be sent!");
            println!(
                "⚠️  Security Credential (first 50 chars): {}...",
                &self.config.mpesa_security_credential
                    [..50.min(self.config.mpesa_security_credential.len())]
            );

            // Validate security credential length for production
            if self.config.mpesa_security_credential.len() < 100 {
                error!("❌ CRITICAL: Production security credential is too short!");
                error!("   Expected: ~600 chars (encrypted RSA)");
                error!(
                    "   Got: {} chars",
                    self.config.mpesa_security_credential.len()
                );
                return Err("Invalid production security credential".into());
            }
        }

        let amount_parsed = amount.parse::<f64>()?;
        if amount_parsed <= 0.0 {
            return Err("Amount must be greater than 0".into());
        }

        // Check minimum amount for B2C
        if amount_parsed < 10.0 {
            return Err("Minimum B2C amount is KSh 10".into());
        }

        let access_token = self.get_access_token().await?;
        let formatted_phone = self.format_phone_number(phone_number);

        println!("[B2C] Formatted Phone: {}", formatted_phone);
        println!("[B2C] Access Token: {}...", &access_token[..20]);

        let (_, _, b2c_url) = self.config.get_mpesa_urls();
        println!("[B2C] API URL: {}", b2c_url);

        let b2c_request = B2CRequest {
            initiator_name: self.config.mpesa_initiator_name.clone(),
            security_credential: self.config.mpesa_security_credential.clone(),
            command_id: command_id.to_string(),
            amount: amount.to_string(),
            party_a: self.config.mpesa_short_code.clone(),
            party_b: formatted_phone.clone(),
            remarks: remarks.to_string(),
            queue_timeout_url: self.config.mpesa_b2c_queue_timeout_url.clone(),
            result_url: self.config.mpesa_b2c_result_url.clone(),
            occasion: occasion.map(|s| s.to_string()),
        };

        println!("[B2C] Request body:");
        println!("  Initiator: {}", b2c_request.initiator_name);
        println!("  Command ID: {}", b2c_request.command_id);
        println!("  Amount: {}", b2c_request.amount);
        println!("  Party A (Business): {}", b2c_request.party_a);
        println!("  Party B (Customer): {}", b2c_request.party_b);
        println!("  Result URL: {}", b2c_request.result_url);
        println!("  Timeout URL: {}", b2c_request.queue_timeout_url);

        let response = self
            .client
            .post(&b2c_url)
            .header(header::AUTHORIZATION, format!("Bearer {}", access_token))
            .header(header::CONTENT_TYPE, "application/json")
            .json(&b2c_request)
            .send()
            .await?;

        let status = response.status();
        let response_text = response.text().await?;

        println!("[B2C] Response Status: {}", status);
        println!("[B2C] Response Body: {}", response_text);

        if !status.is_success() {
            error!("B2C failed: {} - {}", status, response_text);
            return Err(format!("B2C failed: {} - {}", status, response_text).into());
        }

        let b2c_response: B2CResponse = serde_json::from_str(&response_text)?;

        println!("==========================================");
        println!("✅ B2C TRANSACTION RESPONSE");
        println!("Conversation ID: {}", b2c_response.conversation_id);
        println!(
            "Originator Conversation ID: {}",
            b2c_response.originator_conversation_id
        );
        println!("Response Code: {}", b2c_response.response_code);
        println!(
            "Response Description: {}",
            b2c_response.response_description
        );
        println!("==========================================");

        info!("B2C initiated: {}", b2c_response.conversation_id);

        // Check if M-Pesa accepted the request
        if b2c_response.response_code != "0" {
            error!(
                "M-Pesa rejected B2C request: {}",
                b2c_response.response_description
            );
            return Err(format!("M-Pesa error: {}", b2c_response.response_description).into());
        }

        Ok(b2c_response)
    }

    pub async fn check_connectivity(
        &self,
    ) -> Result<ConnectivityStatus, Box<dyn std::error::Error>> {
        println!("[CONNECTIVITY] Checking M-Pesa connectivity...");

        let (auth_url, _, _) = self.config.get_mpesa_urls();
        println!("[CONNECTIVITY] Testing auth endpoint: {}", auth_url);

        match self.get_access_token().await {
            Ok(_token) => {
                println!("[CONNECTIVITY] ✓ Access token obtained successfully");
                Ok(ConnectivityStatus {
                    connected: true,
                    environment: self.config.mpesa_environment.clone(),
                    business_shortcode: self.config.mpesa_short_code.clone(),
                    can_send_b2c: true,
                    account_balance: "Unknown".to_string(),
                })
            }
            Err(e) => {
                println!("[CONNECTIVITY] ✗ Failed to get access token: {}", e);
                Ok(ConnectivityStatus {
                    connected: false,
                    environment: self.config.mpesa_environment.clone(),
                    business_shortcode: self.config.mpesa_short_code.clone(),
                    can_send_b2c: false,
                    account_balance: "Unknown".to_string(),
                })
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConnectivityStatus {
    pub connected: bool,
    pub environment: String,
    pub business_shortcode: String,
    pub can_send_b2c: bool,
    pub account_balance: String,
}
