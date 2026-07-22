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

        Ok(Self {
            client: Client::new(),
            bucket_name,
            api_key,
        })
    }

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

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?uploadType=media&key={}",
            self.bucket_name, &path, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Upload failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::InternalServerError(format!(
                "Upload failed: {} - {}",
                status, text
            )));
        }

        let download_url = self.get_download_url(&path).await?;
        let public_id: String = path;

        Ok((download_url, public_id))
    }

    pub async fn upload_thumbnail(&self, data: &[u8], user_id: &str) -> Result<String, AppError> {
        let path = format!("videos/{}/thumb_{}", user_id, Uuid::new_v4());

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?uploadType=media&key={}",
            self.bucket_name, &path, self.api_key
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
            return Err(AppError::InternalServerError(
                "Thumbnail upload failed".to_string(),
            ));
        }

        let download_url = self.get_download_url(&path).await?;
        Ok(download_url)
    }

    pub async fn get_download_url(&self, path: &str) -> Result<String, AppError> {
        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media&key={}",
            self.bucket_name, path, self.api_key
        );
        Ok(url)
    }

    pub async fn delete_file(&self, public_id: &str) -> Result<(), AppError> {
        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?key={}",
            self.bucket_name, public_id, self.api_key
        );

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Delete failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::InternalServerError("Delete failed".to_string()));
        }

        Ok(())
    }
}
