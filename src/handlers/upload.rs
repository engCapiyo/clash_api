use axum::{extract::Path, http::StatusCode, response::Response};
use std::path::Path as StdPath;
use tokio_util::io::ReaderStream;

use crate::errors::{AppError, Result};

// ============================================================================
// SERVE IMAGE
// ============================================================================

pub async fn serve_image(Path(file_name): Path<String>) -> Result<Response> {
    // Security: prevent path traversal
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err(AppError::PostNotFound);
    }

    let file_path = format!("uploads/images/{}", file_name);

    // Check if file exists and is a file (not a directory)
    if !StdPath::new(&file_path).is_file() {
        return Err(AppError::PostNotFound);
    }

    // Try to open the file
    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| AppError::PostNotFound)?;

    // Convert the file into a Stream
    let stream = ReaderStream::new(file);

    // Set appropriate content type based on file extension
    let content_type = if file_path.ends_with(".png") {
        "image/png"
    } else if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") {
        "image/jpeg"
    } else if file_path.ends_with(".gif") {
        "image/gif"
    } else if file_path.ends_with(".webp") {
        "image/webp"
    } else if file_path.ends_with(".bmp") {
        "image/bmp"
    } else if file_path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=31536000")
        .body(axum::body::Body::from_stream(stream))
        .unwrap();

    Ok(response)
}

// ============================================================================
// SERVE VIDEO (NEW)
// ============================================================================

pub async fn serve_video(Path(file_name): Path<String>) -> Result<Response> {
    // Security: prevent path traversal
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err(AppError::PostNotFound);
    }

    let file_path = format!("uploads/videos/{}", file_name);

    // Check if file exists and is a file (not a directory)
    if !StdPath::new(&file_path).is_file() {
        return Err(AppError::PostNotFound);
    }

    // Try to open the file
    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| AppError::PostNotFound)?;

    // Convert the file into a Stream
    let stream = ReaderStream::new(file);

    // Set appropriate content type based on file extension
    let content_type = if file_path.ends_with(".mp4") {
        "video/mp4"
    } else if file_path.ends_with(".mov") {
        "video/quicktime"
    } else if file_path.ends_with(".avi") {
        "video/x-msvideo"
    } else if file_path.ends_with(".mkv") {
        "video/x-matroska"
    } else if file_path.ends_with(".webm") {
        "video/webm"
    } else if file_path.ends_with(".flv") {
        "video/x-flv"
    } else if file_path.ends_with(".wmv") {
        "video/x-ms-wmv"
    } else if file_path.ends_with(".m4v") {
        "video/x-m4v"
    } else if file_path.ends_with(".mpg") || file_path.ends_with(".mpeg") {
        "video/mpeg"
    } else {
        "video/mp4" // Default fallback
    };

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=31536000")
        .header("accept-ranges", "bytes") // Support for seeking in video
        .body(axum::body::Body::from_stream(stream))
        .unwrap();

    Ok(response)
}

// ============================================================================
// SERVE ANY MEDIA (Auto-detect based on file extension)
// ============================================================================

pub async fn serve_media(Path(file_name): Path<String>) -> Result<Response> {
    // Security: prevent path traversal
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return Err(AppError::PostNotFound);
    }

    // Determine file type by extension
    let is_video = file_name.ends_with(".mp4")
        || file_name.ends_with(".mov")
        || file_name.ends_with(".avi")
        || file_name.ends_with(".mkv")
        || file_name.ends_with(".webm")
        || file_name.ends_with(".flv")
        || file_name.ends_with(".wmv")
        || file_name.ends_with(".m4v")
        || file_name.ends_with(".mpg")
        || file_name.ends_with(".mpeg");

    let is_image = file_name.ends_with(".png")
        || file_name.ends_with(".jpg")
        || file_name.ends_with(".jpeg")
        || file_name.ends_with(".gif")
        || file_name.ends_with(".webp")
        || file_name.ends_with(".bmp")
        || file_name.ends_with(".svg");

    let file_path = if is_video {
        format!("uploads/videos/{}", file_name)
    } else if is_image {
        format!("uploads/images/{}", file_name)
    } else {
        return Err(AppError::PostNotFound);
    };

    // Check if file exists
    if !StdPath::new(&file_path).is_file() {
        return Err(AppError::PostNotFound);
    }

    // Open the file
    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| AppError::PostNotFound)?;

    let stream = ReaderStream::new(file);

    // Set content type
    let content_type = if is_video {
        if file_path.ends_with(".mp4") {
            "video/mp4"
        } else if file_path.ends_with(".mov") {
            "video/quicktime"
        } else if file_path.ends_with(".webm") {
            "video/webm"
        } else {
            "video/mp4"
        }
    } else {
        if file_path.ends_with(".png") {
            "image/png"
        } else if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") {
            "image/jpeg"
        } else if file_path.ends_with(".gif") {
            "image/gif"
        } else {
            "application/octet-stream"
        }
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=31536000");

    // Add range headers for video seeking
    if is_video {
        builder = builder.header("accept-ranges", "bytes");
    }

    let response = builder.body(axum::body::Body::from_stream(stream)).unwrap();

    Ok(response)
}
