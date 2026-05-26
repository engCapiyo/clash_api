use dashmap::DashMap;
use mongodb::Database;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::errors::AppError;
use crate::services::cloudinary::CloudinaryService;
use crate::services::fcm_service::FCMService;
use crate::services::mpesa_service::MpesaService;

/// One broadcast channel per fixtureId.
/// Key = fixtureId, Value = sender half of the channel.
pub type CommentBroadcaster = Arc<DashMap<String, broadcast::Sender<String>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub mpesa_service: Option<Arc<MpesaService>>,
    pub fcm_service: Option<Arc<FCMService>>,
    pub cloudinary: CloudinaryService,
    /// Shared in-memory broadcaster — no Redis needed
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

    /// Get or create a broadcast sender for a given fixtureId.
    pub fn get_or_create_broadcaster(&self, fixture_id: &str) -> broadcast::Sender<String> {
        if let Some(tx) = self.comment_broadcaster.get(fixture_id) {
            return tx.clone();
        }
        // capacity 64: if a slow client misses messages that's fine
        let (tx, _) = broadcast::channel(64);
        self.comment_broadcaster
            .insert(fixture_id.to_string(), tx.clone());
        tx
    }
}
