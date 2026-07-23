use chrono::{DateTime, Utc};
use mongodb::bson;
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Post {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,

    pub user_id: String,
    pub user_name: String,

    // Text
    pub caption: Option<String>,

    // Image (Cloudinary)
    pub image_url: Option<String>,
    pub cloudinary_public_id: Option<String>,
    pub image_format: Option<String>,

    // ✅ NEW: Image (Firebase Storage) - for backup/storage
    pub firebase_image_url: Option<String>,
    pub firebase_image_public_id: Option<String>,

    // Video (Firebase Storage)
    pub video_url: Option<String>,
    pub video_thumbnail_url: Option<String>,
    pub video_duration: Option<i32>,        // seconds
    pub video_size: Option<i64>,            // bytes
    pub firebase_public_id: Option<String>, // Firebase storage path

    pub likes_count: i32,
    pub comments_count: i32,
    pub shares_count: i32,
    pub liked_by: Vec<String>,
    pub is_saved: bool,

    pub post_type: PostType,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PostType {
    Text,
    Image,
    Video,
    TextAndImage,
    TextAndVideo,
}

impl Post {
    pub fn new_text_post(user_id: String, user_name: String, caption: String) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            user_id,
            user_name,
            caption: Some(caption),
            image_url: None,
            cloudinary_public_id: None,
            image_format: None,
            firebase_image_url: None,
            firebase_image_public_id: None,
            video_url: None,
            video_thumbnail_url: None,
            video_duration: None,
            video_size: None,
            firebase_public_id: None,
            likes_count: 0,
            comments_count: 0,
            shares_count: 0,
            liked_by: Vec::new(),
            is_saved: false,
            post_type: PostType::Text,
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn new_image_post(
        user_id: String,
        user_name: String,
        image_url: String,
        cloudinary_public_id: String,
        image_format: String,
        firebase_image_url: Option<String>,
        firebase_image_public_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            user_id,
            user_name,
            caption: None,
            image_url: Some(image_url),
            cloudinary_public_id: Some(cloudinary_public_id),
            image_format: Some(image_format),
            firebase_image_url,
            firebase_image_public_id,
            video_url: None,
            video_thumbnail_url: None,
            video_duration: None,
            video_size: None,
            firebase_public_id: None,
            likes_count: 0,
            comments_count: 0,
            shares_count: 0,
            liked_by: Vec::new(),
            is_saved: false,
            post_type: PostType::Image,
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn new_text_image_post(
        user_id: String,
        user_name: String,
        caption: String,
        image_url: String,
        cloudinary_public_id: String,
        image_format: String,
        firebase_image_url: Option<String>,
        firebase_image_public_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            user_id,
            user_name,
            caption: Some(caption),
            image_url: Some(image_url),
            cloudinary_public_id: Some(cloudinary_public_id),
            image_format: Some(image_format),
            firebase_image_url,
            firebase_image_public_id,
            video_url: None,
            video_thumbnail_url: None,
            video_duration: None,
            video_size: None,
            firebase_public_id: None,
            likes_count: 0,
            comments_count: 0,
            shares_count: 0,
            liked_by: Vec::new(),
            is_saved: false,
            post_type: PostType::TextAndImage,
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn new_video_post(
        user_id: String,
        user_name: String,
        video_url: String,
        firebase_public_id: String,
        video_thumbnail_url: Option<String>,
        video_duration: Option<i32>,
        video_size: Option<i64>,
    ) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            user_id,
            user_name,
            caption: None,
            image_url: None,
            cloudinary_public_id: None,
            image_format: None,
            firebase_image_url: None,
            firebase_image_public_id: None,
            video_url: Some(video_url),
            video_thumbnail_url,
            video_duration,
            video_size,
            firebase_public_id: Some(firebase_public_id),
            likes_count: 0,
            comments_count: 0,
            shares_count: 0,
            liked_by: Vec::new(),
            is_saved: false,
            post_type: PostType::Video,
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn new_text_video_post(
        user_id: String,
        user_name: String,
        caption: String,
        video_url: String,
        firebase_public_id: String,
        video_thumbnail_url: Option<String>,
        video_duration: Option<i32>,
        video_size: Option<i64>,
    ) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            user_id,
            user_name,
            caption: Some(caption),
            image_url: None,
            cloudinary_public_id: None,
            image_format: None,
            firebase_image_url: None,
            firebase_image_public_id: None,
            video_url: Some(video_url),
            video_thumbnail_url,
            video_duration,
            video_size,
            firebase_public_id: Some(firebase_public_id),
            likes_count: 0,
            comments_count: 0,
            shares_count: 0,
            liked_by: Vec::new(),
            is_saved: false,
            post_type: PostType::TextAndVideo,
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn has_image(&self) -> bool {
        self.image_url.is_some()
    }

    pub fn has_video(&self) -> bool {
        self.video_url.is_some()
    }

    pub fn has_text(&self) -> bool {
        self.caption.is_some()
    }

    pub fn update_timestamps(&mut self) {
        let now = Utc::now();
        self.updated_at = now;
        self.last_modified = now;
    }
}

// ============================================================================
// COMMENT MODEL WITH PARENT_COMMENT_ID FOR REPLIES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct Comment {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,

    pub post_id: String,
    pub user_id: String,
    pub user_name: String,

    #[validate(length(min = 1, message = "Comment cannot be empty"))]
    pub comment: String,

    // ✅ NEW: Parent comment ID for reply support
    pub parent_comment_id: Option<String>,

    pub likes_count: i32,
    pub liked_by: Vec<String>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,

    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub last_modified: DateTime<Utc>,
}

impl Comment {
    pub fn new(
        post_id: String,
        user_id: String,
        user_name: String,
        comment: String,
        parent_comment_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            _id: Some(ObjectId::new()),
            post_id,
            user_id,
            user_name,
            comment,
            parent_comment_id,
            likes_count: 0,
            liked_by: Vec::new(),
            created_at: now,
            updated_at: now,
            last_modified: now,
        }
    }

    pub fn update_timestamps(&mut self) {
        let now = Utc::now();
        self.updated_at = now;
        self.last_modified = now;
    }
}

// ============================================================================
// POST RESPONSE
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct PostResponse {
    pub id: String,
    pub user_id: String,
    pub user_name: String,
    pub caption: Option<String>,
    pub image_url: Option<String>,
    pub cloudinary_public_id: Option<String>,
    pub image_format: Option<String>,
    pub firebase_image_url: Option<String>,
    pub firebase_image_public_id: Option<String>,
    pub video_url: Option<String>,
    pub video_thumbnail_url: Option<String>,
    pub video_duration: Option<i32>,
    pub video_size: Option<i64>,
    pub firebase_public_id: Option<String>,
    pub post_type: String,
    pub likes_count: i32,
    pub comments_count: i32,
    pub shares_count: i32,
    pub liked_by: Vec<String>,
    pub is_saved: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_modified: String,
    pub timestamp: i64,
}

impl From<Post> for PostResponse {
    fn from(post: Post) -> Self {
        let post_type_str = match post.post_type {
            PostType::Text => "text".to_string(),
            PostType::Image => "image".to_string(),
            PostType::Video => "video".to_string(),
            PostType::TextAndImage => "text_and_image".to_string(),
            PostType::TextAndVideo => "text_and_video".to_string(),
        };

        PostResponse {
            id: post._id.unwrap().to_hex(),
            user_id: post.user_id,
            user_name: post.user_name,
            caption: post.caption,
            image_url: post.image_url,
            cloudinary_public_id: post.cloudinary_public_id,
            image_format: post.image_format,
            firebase_image_url: post.firebase_image_url,
            firebase_image_public_id: post.firebase_image_public_id,
            video_url: post.video_url,
            video_thumbnail_url: post.video_thumbnail_url,
            video_duration: post.video_duration,
            video_size: post.video_size,
            firebase_public_id: post.firebase_public_id,
            post_type: post_type_str,
            likes_count: post.likes_count,
            comments_count: post.comments_count,
            shares_count: post.shares_count,
            liked_by: post.liked_by,
            is_saved: post.is_saved,
            created_at: post.created_at.to_rfc3339(),
            updated_at: post.updated_at.to_rfc3339(),
            last_modified: post.last_modified.to_rfc3339(),
            timestamp: post.last_modified.timestamp(),
        }
    }
}

// ============================================================================
// COMMENT RESPONSE WITH PARENT_COMMENT_ID
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentResponse {
    pub id: String,
    pub post_id: String,
    pub user_id: String,
    pub user_name: String,
    pub comment: String,
    // ✅ NEW: Parent comment ID for reply support
    pub parent_comment_id: Option<String>,
    pub likes_count: i32,
    pub liked_by: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_modified: String,
    pub timestamp: i64,
}

impl From<Comment> for CommentResponse {
    fn from(comment: Comment) -> Self {
        CommentResponse {
            id: comment._id.unwrap().to_hex(),
            post_id: comment.post_id,
            user_id: comment.user_id,
            user_name: comment.user_name,
            comment: comment.comment,
            parent_comment_id: comment.parent_comment_id,
            likes_count: comment.likes_count,
            liked_by: comment.liked_by,
            created_at: comment.created_at.to_rfc3339(),
            updated_at: comment.updated_at.to_rfc3339(),
            last_modified: comment.last_modified.to_rfc3339(),
            timestamp: comment.last_modified.timestamp(),
        }
    }
}

// ============================================================================
// REQUEST STRUCTS
// ============================================================================

#[derive(Debug, Deserialize, Serialize)]
pub struct LikeRequest {
    pub user_id: String,
    pub user_name: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateCommentRequest {
    pub user_id: String,
    pub user_name: String,
    #[validate(length(min = 1, message = "Comment cannot be empty"))]
    pub comment: String,
    // ✅ NEW: Parent comment ID for reply support
    pub parent_comment_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateCommentRequest {
    pub user_id: String,
    #[validate(length(min = 1, message = "Comment cannot be empty"))]
    pub comment: String,
}
