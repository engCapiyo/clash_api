use dashmap::DashMap;
use mongodb::{Client, Database};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::errors::AppError;
use crate::services::cloudinary::CloudinaryService;
use crate::services::fcm_service::FCMService;
use crate::services::mpesa_service::MpesaService;
use crate::services::storage_service::StorageService; // ✅ ADDED

/// One broadcast channel per room key.
/// Key format: "{channel_id}_{fixture_id}" or "{channel_id}_overall"
pub type CommentBroadcaster = Arc<DashMap<String, broadcast::Sender<String>>>;

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub db: Database,
    pub mpesa_service: Option<Arc<MpesaService>>,
    pub fcm_service: Option<Arc<FCMService>>,
    pub cloudinary: CloudinaryService,
    pub storage_service: StorageService, // ✅ ADDED
    pub comment_broadcaster: CommentBroadcaster,
    pub active_users: Arc<DashMap<String, bool>>,
}

impl AppState {
    // ⚠️ now async — StorageService::new() is async because it fetches a
    // GCP OAuth2 token via gcp_auth at construction time. The caller in
    // main.rs must `.await` this.
    pub async fn new(client: Client, db: Database) -> Result<Self, AppError> {
        let cloudinary = CloudinaryService::new()?;
        let storage_service = StorageService::new().await?; // ✅ now awaited

        Ok(AppState {
            client,
            db,
            mpesa_service: None,
            fcm_service: None,
            cloudinary,
            storage_service, // ✅ ADDED
            comment_broadcaster: Arc::new(DashMap::new()),
            active_users: Arc::new(DashMap::new()),
        })
    }

    pub fn with_mpesa(mut self, mpesa_service: Arc<MpesaService>) -> Self {
        self.mpesa_service = Some(mpesa_service);
        self
    }

    pub fn with_fcm(mut self, fcm_service: Arc<FCMService>) -> Self {
        self.fcm_service = Some(fcm_service);
        self
    }

    /// Get or create a broadcast sender for a given room key.
    /// Room key format: "{channel_id}_{fixture_id}" or "{channel_id}_overall"
    pub fn get_or_create_broadcaster(&self, room_key: &str) -> broadcast::Sender<String> {
        if let Some(tx) = self.comment_broadcaster.get(room_key) {
            return tx.clone();
        }
        let (tx, _) = broadcast::channel(64);
        self.comment_broadcaster
            .insert(room_key.to_string(), tx.clone());
        tx
    }

    /// Mark user as online
    pub fn set_user_online(&self, user_id: &str) {
        self.active_users.insert(user_id.to_string(), true);
        tracing::debug!("✅ User {} is now online", user_id);
    }

    /// Mark user as offline
    pub fn set_user_offline(&self, user_id: &str) {
        self.active_users.remove(user_id);
        tracing::debug!("❌ User {} is now offline", user_id);
    }

    /// Check if user is online
    pub fn is_user_online(&self, user_id: &str) -> bool {
        self.active_users.get(user_id).is_some()
    }

    /// Get all online users
    pub fn get_online_users(&self) -> Vec<String> {
        self.active_users
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}
