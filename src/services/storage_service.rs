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
        println!("✅ API Key length: {}", api_key.len());

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

        // ✅ URL encode the path
        let encoded_path = urlencoding::encode(&path);

        println!("📤 Uploading video to Firebase Storage");
        println!("   Path: {}", path);
        println!("   Encoded Path: {}", encoded_path);
        println!("   Size: {} bytes", data.len());

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?uploadType=media&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!("   URL: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                println!("❌ Reqwest error: {}", e);
                AppError::InternalServerError(format!("Video upload failed: {}", e))
            })?;

        let status = response.status();
        println!("   Response Status: {}", status);

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            println!("❌ Firebase Error Response: {}", text);
            return Err(AppError::InternalServerError(format!(
                "Video upload failed: {} - {}",
                status, text
            )));
        }

        let download_url = self.get_download_url(&path).await?;
        println!("✅ Video uploaded successfully: {}", download_url);

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

        // ✅ URL encode the path
        let encoded_path = urlencoding::encode(&path);

        println!("📤 Uploading image to Firebase Storage");
        println!("   Path: {}", path);
        println!("   Encoded Path: {}", encoded_path);
        println!("   Size: {} bytes", data.len());

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?uploadType=media&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!("   URL: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                println!("❌ Reqwest error: {}", e);
                AppError::InternalServerError(format!("Image upload failed: {}", e))
            })?;

        let status = response.status();
        println!("   Response Status: {}", status);

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            println!("❌ Firebase Error Response: {}", text);
            return Err(AppError::InternalServerError(format!(
                "Image upload failed: {} - {}",
                status, text
            )));
        }

        let download_url = self.get_download_url(&path).await?;
        println!("✅ Image uploaded successfully: {}", download_url);

        Ok((download_url, path))
    }

    // ============================================================================
    // THUMBNAIL UPLOAD
    // ============================================================================
    pub async fn upload_thumbnail(&self, data: &[u8], user_id: &str) -> Result<String, AppError> {
        let path = format!("videos/{}/thumb_{}", user_id, Uuid::new_v4());

        // ✅ URL encode the path
        let encoded_path = urlencoding::encode(&path);

        println!("📤 Uploading thumbnail to Firebase Storage");
        println!("   Path: {}", path);
        println!("   Encoded Path: {}", encoded_path);
        println!("   Size: {} bytes", data.len());

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?uploadType=media&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!("   URL: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "image/jpeg")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                println!("❌ Reqwest error: {}", e);
                AppError::InternalServerError(format!("Thumbnail upload failed: {}", e))
            })?;

        let status = response.status();
        println!("   Response Status: {}", status);

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            println!("❌ Firebase Error Response: {}", text);
            return Err(AppError::InternalServerError(format!(
                "Thumbnail upload failed: {} - {}",
                status, text
            )));
        }

        let download_url = self.get_download_url(&path).await?;
        println!("✅ Thumbnail uploaded successfully: {}", download_url);

        Ok(download_url)
    }

    // ============================================================================
    // GET DOWNLOAD URL
    // ============================================================================
    pub async fn get_download_url(&self, path: &str) -> Result<String, AppError> {
        // ✅ URL encode the path for download URL too
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
        // ✅ URL encode the public_id for delete too
        let encoded_id = urlencoding::encode(public_id);

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?key={}",
            self.bucket_name, encoded_id, self.api_key
        );

        println!("🔍 Deleting file: {}", public_id);
        println!("   URL: {}", url);

        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::InternalServerError(format!("Delete failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            println!("❌ Delete failed: {} - {}", status, text);
            return Err(AppError::InternalServerError(format!(
                "Delete failed: {} - {}",
                status, text
            )));
        }

        println!("✅ File deleted successfully");
        Ok(())
    }
}
