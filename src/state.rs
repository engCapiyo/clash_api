use dashmap::DashMap;
use mongodb::Database;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::errors::AppError;
use crate::services::cloudinary::CloudinaryService;
use crate::services::fcm_service::FCMService;
use crate::services::mpesa_service::MpesaService;

/// One broadcast channel per room key.
/// Key format: "{channel_id}_{fixture_id}" or "{channel_id}_overall"
pub type CommentBroadcaster = Arc<DashMap<String, broadcast::Sender<String>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub mpesa_service: Option<Arc<MpesaService>>,
    pub fcm_service: Option<Arc<FCMService>>,
    pub cloudinary: CloudinaryService,
    pub comment_broadcaster: CommentBroadcaster,
}

impl AppState {
    pub fn new(db: Database) -> Result<Self, AppError> {
        let cloudinary = CloudinaryService::new()?;

        Ok(AppState {
            db,
            mpesa_service: None,
            fcm_service: None,
            cloudinary,
            comment_broadcaster: Arc::new(DashMap::new()),
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
        self.comment_broadcaster.insert(room_key.to_string(), tx.clone());
        tx
    }
}