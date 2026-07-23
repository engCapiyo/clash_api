use chrono::Utc;
use gcp_auth::{AuthenticationManager, CustomServiceAccount};
use reqwest::Client;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::errors::AppError;

const STORAGE_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_write";

#[derive(Clone)]
pub struct StorageService {
    pub client: Client,
    pub bucket_name: String,
    pub auth: Arc<AuthenticationManager>,
}

impl StorageService {
    pub async fn new() -> Result<Self, AppError> {
        let bucket_name = env::var("FIREBASE_STORAGE_BUCKET").map_err(|_| {
            AppError::InternalServerError("FIREBASE_STORAGE_BUCKET not set".to_string())
        })?;

        // Read the service account JSON directly from an env var (pasted as
        // its value on Render) instead of expecting a mounted file path.
        // No API key involved — auth is via OAuth2 Bearer token, which is
        // what the real GCS JSON API expects.
        let credentials_json = env::var("GOOGLE_SERVICE_ACCOUNT_JSON").map_err(|_| {
            AppError::InternalServerError("GOOGLE_SERVICE_ACCOUNT_JSON not set".to_string())
        })?;

        let service_account = CustomServiceAccount::from_json(&credentials_json).map_err(|e| {
            AppError::InternalServerError(format!(
                "Failed to parse GOOGLE_SERVICE_ACCOUNT_JSON: {}",
                e
            ))
        })?;

        let auth = AuthenticationManager::from(service_account);

        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to build HTTP client: {}", e))
            })?;

        println!(
            "✅ StorageService initialized with bucket: {} (GCS JSON API + service account)",
            bucket_name
        );

        Ok(Self {
            client,
            bucket_name,
            auth: Arc::new(auth),
        })
    }

    async fn access_token(&self) -> Result<String, AppError> {
        let token = self.auth.get_token(&[STORAGE_SCOPE]).await.map_err(|e| {
            eprintln!("❌ Failed to get GCP access token: {}", e);
            AppError::InternalServerError(format!("Failed to get access token: {}", e))
        })?;
        Ok(token.as_str().to_string())
    }

    fn image_content_type(file_extension: &str) -> &'static str {
        match file_extension.to_lowercase().as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            _ => "image/jpeg",
        }
    }

    fn video_content_type(file_extension: &str) -> &'static str {
        match file_extension.to_lowercase().as_str() {
            "mov" => "video/quicktime",
            "avi" => "video/x-msvideo",
            "mkv" => "video/x-matroska",
            _ => "video/mp4",
        }
    }

    async fn upload_object(
        &self,
        path: &str,
        data: &[u8],
        content_type: &str,
        log_tag: &str,
    ) -> Result<(), AppError> {
        let encoded_path = urlencoding::encode(path);
        let token = self.access_token().await?;

        // Real GCS JSON API "simple upload" endpoint. This is IAM-authenticated
        // (via the Bearer token), so it bypasses Firebase Storage Rules and
        // Firebase's client-SDK-only resumable upload protocol entirely.
        let url = format!(
            "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}&predefinedAcl=publicRead",
            self.bucket_name, encoded_path
        );

        println!(
            "🔍 [{}] Starting upload: path={}, size={} bytes",
            log_tag,
            path,
            data.len()
        );

        let response = self
            .client
            .post(&url)
            .bearer_auth(token)
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                eprintln!("❌ [{}] Request failed: {}", log_tag, e);
                AppError::InternalServerError(format!("Upload failed: {}", e))
            })?;

        println!(
            "🔍 [{}] Response received, status: {}",
            log_tag,
            response.status()
        );

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("❌ [{}] Upload failed: {} - {}", log_tag, status, text);
            return Err(AppError::InternalServerError(format!(
                "Upload failed: {} - {}",
                status, text
            )));
        }

        println!("✅ [{}] Upload complete: {}", log_tag, path);

        Ok(())
    }

    fn public_download_url(&self, path: &str) -> String {
        let encoded_path = urlencoding::encode(path);
        format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media",
            self.bucket_name, encoded_path
        )
    }

    // ============================================================================
    // VIDEO UPLOAD
    // ============================================================================
    pub async fn upload_video(
        &self,
        data: &[u8],
        user_id: &str,
        file_extension: &str,
    ) -> Result<(String, String), AppError> {
        let path = format!(
            "videos/{}/{}_{}.{}",
            user_id,
            Uuid::new_v4(),
            Utc::now().timestamp(),
            file_extension
        );

        self.upload_object(
            &path,
            data,
            Self::video_content_type(file_extension),
            "upload_video",
        )
        .await?;

        Ok((self.public_download_url(&path), path))
    }

    // ============================================================================
    // IMAGE UPLOAD
    // ============================================================================
    pub async fn upload_image(
        &self,
        data: &[u8],
        user_id: &str,
        file_extension: &str,
    ) -> Result<(String, String), AppError> {
        let path = format!(
            "images/{}/{}_{}.{}",
            user_id,
            Uuid::new_v4(),
            Utc::now().timestamp(),
            file_extension
        );

        self.upload_object(
            &path,
            data,
            Self::image_content_type(file_extension),
            "upload_image",
        )
        .await?;

        Ok((self.public_download_url(&path), path))
    }

    // ============================================================================
    // THUMBNAIL UPLOAD
    // ============================================================================
    pub async fn upload_thumbnail(&self, data: &[u8], user_id: &str) -> Result<String, AppError> {
        let path = format!("videos/{}/thumb_{}.jpg", user_id, Uuid::new_v4());

        self.upload_object(&path, data, "image/jpeg", "upload_thumbnail")
            .await?;

        Ok(self.public_download_url(&path))
    }

    // ============================================================================
    // GET DOWNLOAD URL
    // ============================================================================
    pub async fn get_download_url(&self, path: &str) -> Result<String, AppError> {
        Ok(self.public_download_url(path))
    }

    // ============================================================================
    // DELETE FILE
    // ============================================================================
    pub async fn delete_file(&self, public_id: &str) -> Result<(), AppError> {
        let encoded_id = urlencoding::encode(public_id);
        let token = self.access_token().await?;

        let url = format!(
            "https://storage.googleapis.com/storage/v1/b/{}/o/{}",
            self.bucket_name, encoded_id
        );

        println!("🔍 [delete_file] Deleting: {}", public_id);

        let response = self
            .client
            .delete(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| {
                eprintln!("❌ [delete_file] Request failed: {}", e);
                AppError::InternalServerError(format!("Delete failed: {}", e))
            })?;

        println!(
            "🔍 [delete_file] Response received, status: {}",
            response.status()
        );

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("❌ [delete_file] Delete failed: {} - {}", status, text);
            return Err(AppError::InternalServerError(format!(
                "Delete failed: {} - {}",
                status, text
            )));
        }

        println!("✅ [delete_file] Deleted: {}", public_id);

        Ok(())
    }
}
