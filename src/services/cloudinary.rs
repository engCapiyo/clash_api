use crate::errors::{AppError, Result};
use reqwest::multipart;
use serde_json::Value;
use std::env;

#[derive(Clone)]
pub struct CloudinaryService {
    cloud_name: String,
    api_key: String,
    api_secret: String,
    upload_preset: String,       // For images
    video_upload_preset: String, // NEW: For videos
}

impl CloudinaryService {
    pub fn new() -> Result<Self> {
        let cloud_name = env::var("CLOUDINARY_CLOUD_NAME").map_err(|e| {
            AppError::CloudinaryError(format!("CLOUDINARY_CLOUD_NAME not set: {}", e))
        })?;

        let api_key = env::var("CLOUDINARY_API_KEY")
            .map_err(|e| AppError::CloudinaryError(format!("CLOUDINARY_API_KEY not set: {}", e)))?;

        let api_secret = env::var("CLOUDINARY_API_SECRET").map_err(|e| {
            AppError::CloudinaryError(format!("CLOUDINARY_API_SECRET not set: {}", e))
        })?;

        let upload_preset = env::var("CLOUDINARY_UPLOAD_PRESET")
            .unwrap_or_else(|_| "rust_backend_upload".to_string());

        // NEW: Read video upload preset from env
        let video_upload_preset =
            env::var("CLOUDINARY_VIDEO_UPLOAD_PRESET").unwrap_or_else(|_| "clash_vida".to_string()); // ← Your video preset

        println!("🔧 Cloudinary Configuration:");
        println!("   Cloud Name: {}", cloud_name);
        println!("   API Key: {}...", &api_key[0..8]);
        println!("   API Secret: {}...", &api_secret[0..8]);
        println!("   Image Upload Preset: {}", upload_preset);
        println!("   Video Upload Preset: {}", video_upload_preset);

        Ok(Self {
            cloud_name,
            api_key,
            api_secret,
            upload_preset,
            video_upload_preset,
        })
    }

    // ============================================================================
    // METHODS FOR CHAT MEDIA (Image & Video)
    // ============================================================================

    /// Upload image for chat messages
    pub async fn upload_image(&self, file_data: Vec<u8>, file_name: &str) -> Result<String> {
        self.upload_to_cloudinary(file_data, file_name, "image", &self.upload_preset)
            .await
    }

    /// Upload video for chat messages
    pub async fn upload_video(&self, file_data: Vec<u8>, file_name: &str) -> Result<String> {
        self.upload_to_cloudinary(file_data, file_name, "video", &self.video_upload_preset)
            .await
    }

    /// Generic upload to Cloudinary for both images and videos
    async fn upload_to_cloudinary(
        &self,
        file_data: Vec<u8>,
        file_name: &str,
        resource_type: &str,
        upload_preset: &str,
    ) -> Result<String> {
        println!("📤 Uploading {} to Cloudinary...", resource_type);
        println!("   File name: {}", file_name);
        println!("   File size: {} bytes", file_data.len());
        println!("   Resource type: {}", resource_type);
        println!("   Upload preset: {}", upload_preset);

        let upload_url = format!(
            "https://api.cloudinary.com/v1_1/{}/{}/upload",
            self.cloud_name, resource_type
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180)) // 3 minutes for videos
            .build()
            .map_err(|e| AppError::CloudinaryError(format!("Failed to create client: {}", e)))?;

        // Determine MIME type
        let mime_type = if resource_type == "image" {
            infer::get(&file_data)
                .map(|info| info.mime_type())
                .unwrap_or("image/jpeg")
        } else {
            if file_name.ends_with(".mp4") {
                "video/mp4"
            } else if file_name.ends_with(".mov") {
                "video/quicktime"
            } else if file_name.ends_with(".webm") {
                "video/webm"
            } else {
                "video/mp4"
            }
        };

        println!("   Detected MIME type: {}", mime_type);

        // Build multipart form with the specific upload preset
        let form = multipart::Form::new()
            .text("upload_preset", upload_preset.to_string())
            .part(
                "file",
                multipart::Part::bytes(file_data)
                    .file_name(file_name.to_string())
                    .mime_str(mime_type)
                    .map_err(|e| {
                        AppError::CloudinaryError(format!("Failed to set MIME type: {}", e))
                    })?,
            );

        // Send request
        let response = client
            .post(&upload_url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::CloudinaryError(format!("Network error: {}", e)))?;

        let status = response.status();
        println!("   Response status: {}", status);

        let response_text = response
            .text()
            .await
            .map_err(|e| AppError::CloudinaryError(format!("Failed to read response: {}", e)))?;

        println!("   Raw response: {}", response_text);

        let result: Value = serde_json::from_str(&response_text).map_err(|e| {
            AppError::CloudinaryError(format!(
                "Failed to parse JSON: {} - Response: {}",
                e, response_text
            ))
        })?;

        // Check for Cloudinary error
        if let Some(error) = result.get("error") {
            let error_msg = error["message"]
                .as_str()
                .unwrap_or("Unknown Cloudinary error");
            println!("❌ Cloudinary error: {}", error_msg);
            return Err(AppError::CloudinaryError(format!(
                "Cloudinary error: {}",
                error_msg
            )));
        }

        // Get secure URL
        let secure_url = result["secure_url"]
            .as_str()
            .ok_or_else(|| {
                println!("❌ No secure_url in response");
                AppError::CloudinaryError("No secure URL in Cloudinary response".into())
            })?
            .to_string();

        println!("✅ Upload successful!");
        println!("   URL: {}", secure_url);

        Ok(secure_url)
    }

    // ============================================================================
    // EXISTING METHODS (Keep all your original code below)
    // ============================================================================

    /// Upload image using upload preset (unsigned - simpler)
    pub async fn upload_image_with_preset(
        &self,
        image_data: &[u8],
        folder: &str,
        public_id: Option<&str>,
    ) -> Result<(String, String)> {
        println!("📤 Starting Cloudinary upload...");
        println!("   Folder: {}", folder);
        println!("   Image size: {} bytes", image_data.len());
        println!("   Upload preset: {}", self.upload_preset);

        let upload_url = format!(
            "https://api.cloudinary.com/v1_1/{}/image/upload",
            self.cloud_name
        );

        println!("   Upload URL: {}", upload_url);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::CloudinaryError(format!("Failed to create client: {}", e)))?;

        let mut form = multipart::Form::new()
            .text("upload_preset", self.upload_preset.clone())
            .text("folder", folder.to_string());

        if let Some(pid) = public_id {
            println!("   Public ID: {}", pid);
            form = form.text("public_id", pid.to_string());
        }

        let mime_type = infer::get(&image_data)
            .map(|info| info.mime_type())
            .unwrap_or("image/jpeg");

        println!("   Detected MIME type: {}", mime_type);

        form = form.part(
            "file",
            multipart::Part::bytes(image_data.to_vec())
                .file_name("upload.jpg")
                .mime_str(mime_type)
                .map_err(|e| {
                    AppError::CloudinaryError(format!("Failed to set MIME type: {}", e))
                })?,
        );

        println!("   Sending request to Cloudinary...");

        let response = client
            .post(&upload_url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::CloudinaryError(format!("Network error: {}", e)))?;

        let status = response.status();
        println!("   Response status: {}", status);

        let response_text = response
            .text()
            .await
            .map_err(|e| AppError::CloudinaryError(format!("Failed to read response: {}", e)))?;

        println!("   Raw response: {}", response_text);

        let result: Value = serde_json::from_str(&response_text).map_err(|e| {
            AppError::CloudinaryError(format!(
                "Failed to parse JSON: {} - Response: {}",
                e, response_text
            ))
        })?;

        println!("   Parsed response: {:?}", result);

        if let Some(error) = result.get("error") {
            let error_msg = error["message"]
                .as_str()
                .unwrap_or("Unknown Cloudinary error");

            println!("❌ Cloudinary error: {}", error_msg);

            if error_msg.contains("Invalid api_key") {
                return Err(AppError::CloudinaryError(
                    "Invalid Cloudinary API key. Please check your credentials.".into(),
                ));
            } else if error_msg.contains("upload preset") {
                return Err(AppError::CloudinaryError(format!(
                    "Invalid upload preset '{}'. Please create it in Cloudinary console.",
                    self.upload_preset
                )));
            }

            return Err(AppError::CloudinaryError(format!(
                "Cloudinary error: {}",
                error_msg
            )));
        }

        let secure_url = result["secure_url"]
            .as_str()
            .ok_or_else(|| {
                println!("❌ No secure_url in response");
                AppError::CloudinaryError("No secure URL in Cloudinary response".into())
            })?
            .to_string();

        let public_id = result["public_id"]
            .as_str()
            .ok_or_else(|| {
                println!("❌ No public_id in response");
                AppError::CloudinaryError("No public ID in Cloudinary response".into())
            })?
            .to_string();

        println!("✅ Upload successful!");
        println!("   URL: {}", secure_url);
        println!("   Public ID: {}", public_id);

        Ok((secure_url, public_id))
    }

    /// Fallback: Upload image using signed upload
    pub async fn upload_image_signed(
        &self,
        image_data: &[u8],
        folder: &str,
        public_id: Option<&str>,
    ) -> Result<(String, String)> {
        println!("🔐 Using signed upload as fallback...");

        let timestamp = chrono::Utc::now().timestamp().to_string();

        let mut params_to_sign = vec![
            format!("folder={}", folder),
            format!("timestamp={}", timestamp),
        ];

        if let Some(pid) = public_id {
            params_to_sign.push(format!("public_id={}", pid));
        }

        params_to_sign.sort();
        let params_string = params_to_sign.join("&");
        let signature_string = format!("{}{}", params_string, self.api_secret);

        let signature = format!("{:x}", md5::compute(signature_string));

        println!("   Parameters: {}", params_string);
        println!("   Signature: {}", signature);

        let upload_url = format!(
            "https://api.cloudinary.com/v1_1/{}/image/upload",
            self.cloud_name
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let mut form = multipart::Form::new()
            .text("api_key", self.api_key.clone())
            .text("timestamp", timestamp)
            .text("signature", signature.clone())
            .text("folder", folder.to_string());

        if let Some(pid) = public_id {
            form = form.text("public_id", pid.to_string());
        }

        form = form.part(
            "file",
            multipart::Part::bytes(image_data.to_vec())
                .file_name("image.jpg")
                .mime_str("image/jpeg")?,
        );

        let response = client.post(&upload_url).multipart(form).send().await?;

        let response_text = response.text().await?;
        println!("   Signed upload response: {}", response_text);

        let result: Value = serde_json::from_str(&response_text)?;

        if let Some(error) = result.get("error") {
            let error_msg = error["message"].as_str().unwrap_or("Unknown error");
            return Err(AppError::CloudinaryError(format!(
                "Signed upload failed: {}",
                error_msg
            )));
        }

        let secure_url = result["secure_url"]
            .as_str()
            .ok_or_else(|| AppError::CloudinaryError("No secure URL".into()))?
            .to_string();

        let public_id = result["public_id"]
            .as_str()
            .ok_or_else(|| AppError::CloudinaryError("No public ID".into()))?
            .to_string();

        println!("✅ Signed upload successful!");
        Ok((secure_url, public_id))
    }

    /// Delete image from Cloudinary
    pub async fn delete_image(&self, public_id: &str) -> Result<()> {
        let timestamp = chrono::Utc::now().timestamp().to_string();

        let signature_data = format!(
            "public_id={}&timestamp={}{}",
            public_id, timestamp, self.api_secret
        );
        let signature = format!("{:x}", md5::compute(signature_data));

        let delete_url = format!(
            "https://api.cloudinary.com/v1_1/{}/image/destroy",
            self.cloud_name
        );

        let params = [
            ("public_id", public_id),
            ("api_key", &self.api_key),
            ("timestamp", &timestamp),
            ("signature", &signature),
        ];

        let client = reqwest::Client::new();
        let response = client.post(&delete_url).form(&params).send().await?;

        let result: Value = response.json().await?;

        if result["result"] != "ok" {
            return Err(AppError::CloudinaryError(format!(
                "Failed to delete image: {}",
                result["result"]
            )));
        }

        Ok(())
    }

    /// Generate transformed URL for image
    pub fn generate_transformed_url(&self, public_id: &str, transformations: &str) -> String {
        format!(
            "https://res.cloudinary.com/{}/image/upload/{}/{}",
            self.cloud_name, transformations, public_id
        )
    }

    /// Generate thumbnail URL for image
    pub fn generate_thumbnail_url(&self, public_id: &str, width: u32, height: u32) -> String {
        let transformations = format!("c_fill,w_{},h_{},q_auto", width, height);
        self.generate_transformed_url(public_id, &transformations)
    }
}
