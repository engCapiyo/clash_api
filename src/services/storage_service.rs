use chrono::Utc;
use reqwest::Client;
use std::env;
use uuid::Uuid;

use crate::errors::AppError;

#[derive(Clone)]
pub struct StorageService {
    pub client: Client,
    pub bucket_name: String,
    pub api_key: String,
}

impl StorageService {
    pub fn new() -> Result<Self, AppError> {
        let bucket_name = env::var("FIREBASE_STORAGE_BUCKET").map_err(|_| {
            AppError::InternalServerError("FIREBASE_STORAGE_BUCKET not set".to_string())
        })?;

        let api_key = env::var("FIREBASE_API_KEY")
            .map_err(|_| AppError::InternalServerError("FIREBASE_API_KEY not set".to_string()))?;

        println!("✅ StorageService initialized with bucket: {}", bucket_name);

        Ok(Self {
            client: Client::new(),
            bucket_name,
            api_key,
        })
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
        let filename = format!(
            "videos/{}/{}_{}",
            user_id,
            Uuid::new_v4(),
            Utc::now().timestamp()
        );
        let path = format!("{}.{}", filename, file_extension);
        let encoded_path = urlencoding::encode(&path);

        let url = format!(
            "https://firebasestorage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&key={}&name={}",
            self.bucket_name, self.api_key, encoded_path
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Video upload failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::InternalServerError(format!(
                "Video upload failed: {} - {}",
                status, text
            )));
        }

        let response_json: serde_json::Value = response.json().await.map_err(|e| {
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = response_json["mediaLink"]
            .as_str()
            .ok_or_else(|| AppError::InternalServerError("No mediaLink in response".to_string()))?
            .to_string();

        Ok((download_url, path))
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
        let filename = format!(
            "images/{}/{}_{}",
            user_id,
            Uuid::new_v4(),
            Utc::now().timestamp()
        );
        let path = format!("{}.{}", filename, file_extension);
        let encoded_path = urlencoding::encode(&path);

        let url = format!(
            "https://firebasestorage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&key={}&name={}",
            self.bucket_name, self.api_key, encoded_path
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Image upload failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::InternalServerError(format!(
                "Image upload failed: {} - {}",
                status, text
            )));
        }

        let response_json: serde_json::Value = response.json().await.map_err(|e| {
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = response_json["mediaLink"]
            .as_str()
            .ok_or_else(|| AppError::InternalServerError("No mediaLink in response".to_string()))?
            .to_string();

        Ok((download_url, path))
    }

    // ============================================================================
    // THUMBNAIL UPLOAD
    // ============================================================================
    pub async fn upload_thumbnail(&self, data: &[u8], user_id: &str) -> Result<String, AppError> {
        let path = format!("videos/{}/thumb_{}", user_id, Uuid::new_v4());
        let encoded_path = urlencoding::encode(&path);

        let url = format!(
            "https://firebasestorage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&key={}&name={}",
            self.bucket_name, self.api_key, encoded_path
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "image/jpeg")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                AppError::InternalServerError(format!("Thumbnail upload failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::InternalServerError(format!(
                "Thumbnail upload failed: {} - {}",
                status, text
            )));
        }

        let response_json: serde_json::Value = response.json().await.map_err(|e| {
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = response_json["mediaLink"]
            .as_str()
            .ok_or_else(|| AppError::InternalServerError("No mediaLink in response".to_string()))?
            .to_string();

        Ok(download_url)
    }

    // ============================================================================
    // GET DOWNLOAD URL
    // ============================================================================
    pub async fn get_download_url(&self, path: &str) -> Result<String, AppError> {
        let encoded_path = urlencoding::encode(path);
        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media&key={}",
            self.bucket_name, encoded_path, self.api_key
        );
        Ok(url)
    }

    // ============================================================================
    // DELETE FILE
    // ============================================================================
    pub async fn delete_file(&self, public_id: &str) -> Result<(), AppError> {
        let encoded_id = urlencoding::encode(public_id);
        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?key={}",
            self.bucket_name, encoded_id, self.api_key
        );

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Delete failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::InternalServerError(format!(
                "Delete failed: {} - {}",
                status, text
            )));
        }

        Ok(())
    }
}
