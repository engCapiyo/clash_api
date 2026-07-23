use chrono::Utc;
use reqwest::Client;
use std::env;
use std::time::Duration;
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

        // Without an explicit timeout, reqwest waits indefinitely on a stalled
        // connection — no error, no log, just a hang. This bounds every
        // request so failures surface instead of disappearing silently.
        let client = Client::builder()
            .timeout(Duration::from_secs(60)) // covers large video bodies
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| {
                AppError::InternalServerError(format!("Failed to build HTTP client: {}", e))
            })?;

        println!("✅ StorageService initialized with bucket: {}", bucket_name);

        Ok(Self {
            client,
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

        // Firebase Storage's own REST API (v0) — no `uploadType` param.
        // That param belongs to the raw GCS JSON API and mixing conventions
        // causes "Invalid HTTP method/URL pair." on this host.
        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o?name={}&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!(
            "🔍 [upload_video] Starting upload: path={}, size={} bytes",
            path,
            data.len()
        );

        let content_type = match file_extension.to_lowercase().as_str() {
            "mov" => "video/quicktime",
            "avi" => "video/x-msvideo",
            "mkv" => "video/x-matroska",
            _ => "video/mp4",
        };

        let response = self
            .client
            .post(&url)
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                eprintln!("❌ [upload_video] Request failed: {}", e);
                AppError::InternalServerError(format!("Video upload failed: {}", e))
            })?;

        println!(
            "🔍 [upload_video] Response received, status: {}",
            response.status()
        );

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("❌ [upload_video] Upload failed: {} - {}", status, text);
            return Err(AppError::InternalServerError(format!(
                "Video upload failed: {} - {}",
                status, text
            )));
        }

        let _response_json: serde_json::Value = response.json().await.map_err(|e| {
            eprintln!("❌ [upload_video] Failed to parse response JSON: {}", e);
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media",
            self.bucket_name, encoded_path
        );

        println!("✅ [upload_video] Upload complete: {}", download_url);

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
            "https://firebasestorage.googleapis.com/v0/b/{}/o?name={}&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!(
            "🔍 [upload_image] Starting upload: path={}, size={} bytes",
            path,
            data.len()
        );

        let content_type = match file_extension.to_lowercase().as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            _ => "image/jpeg",
        };

        let response = self
            .client
            .post(&url)
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                eprintln!("❌ [upload_image] Request failed: {}", e);
                AppError::InternalServerError(format!("Image upload failed: {}", e))
            })?;

        println!(
            "🔍 [upload_image] Response received, status: {}",
            response.status()
        );

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("❌ [upload_image] Upload failed: {} - {}", status, text);
            return Err(AppError::InternalServerError(format!(
                "Image upload failed: {} - {}",
                status, text
            )));
        }

        let _response_json: serde_json::Value = response.json().await.map_err(|e| {
            eprintln!("❌ [upload_image] Failed to parse response JSON: {}", e);
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media",
            self.bucket_name, encoded_path
        );

        println!("✅ [upload_image] Upload complete: {}", download_url);

        Ok((download_url, path))
    }

    // ============================================================================
    // THUMBNAIL UPLOAD
    // ============================================================================
    pub async fn upload_thumbnail(&self, data: &[u8], user_id: &str) -> Result<String, AppError> {
        // Kept under videos/ with a thumb_ prefix to match the Storage Rules
        // that use fileName.matches('thumb_.*') to allow image content here.
        let path = format!("videos/{}/thumb_{}.jpg", user_id, Uuid::new_v4());
        let encoded_path = urlencoding::encode(&path);

        let url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o?name={}&key={}",
            self.bucket_name, encoded_path, self.api_key
        );

        println!(
            "🔍 [upload_thumbnail] Starting upload: path={}, size={} bytes",
            path,
            data.len()
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "image/jpeg")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| {
                eprintln!("❌ [upload_thumbnail] Request failed: {}", e);
                AppError::InternalServerError(format!("Thumbnail upload failed: {}", e))
            })?;

        println!(
            "🔍 [upload_thumbnail] Response received, status: {}",
            response.status()
        );

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("❌ [upload_thumbnail] Upload failed: {} - {}", status, text);
            return Err(AppError::InternalServerError(format!(
                "Thumbnail upload failed: {} - {}",
                status, text
            )));
        }

        let _response_json: serde_json::Value = response.json().await.map_err(|e| {
            eprintln!("❌ [upload_thumbnail] Failed to parse response JSON: {}", e);
            AppError::InternalServerError(format!("Failed to parse response: {}", e))
        })?;

        let download_url = format!(
            "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media",
            self.bucket_name, encoded_path
        );

        println!("✅ [upload_thumbnail] Upload complete: {}", download_url);

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

        println!("🔍 [delete_file] Deleting: {}", public_id);

        let response = self.client.delete(&url).send().await.map_err(|e| {
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
