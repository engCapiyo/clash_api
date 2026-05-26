use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::Utc;
use mongodb::{
    bson::{doc, oid::ObjectId},
    Collection,
};
use futures_util::TryStreamExt;
use serde::Deserialize;
use regex::escape;

use crate::{
    errors::{AppError, Result},
    models::archive::{
        ArchiveActivity, ArchiveActivityRequest, ArchiveActivityResponse, ArchiveQueryParams,
        ActivityType, UserArchiveStats, GetAllArchiveQuery, PaginatedArchiveResponse,
        PaginationInfo, ArchiveStats, AppliedFilters, ActivityBreakdown,
    },
    state::AppState,
};

// POST /api/archive/activity
pub async fn create_archive_activity(
    State(state): State<AppState>,
    Json(payload): Json<ArchiveActivityRequest>,
) -> Result<Json<ArchiveActivityResponse>> {
    println!("📥 Received archive activity: {:?}", payload);

    // Validate activity type
    let activity_type = match payload.activity_type.as_str() {
        "vote" => ActivityType::Vote,
        "like" => ActivityType::Like,
        "comment" => ActivityType::Comment,
        _ => {
            return Err(AppError::invalid_data(
                "Invalid activity type. Must be 'vote', 'like', or 'comment'",
            ));
        }
    };

    // Validate required fields based on activity type
    match activity_type {
        ActivityType::Vote => {
            if payload.selection.is_none() {
                return Err(AppError::invalid_data(
                    "Selection is required for vote activities",
                ));
            }
        }
        ActivityType::Like => {
            if payload.is_liked.is_none() {
                return Err(AppError::invalid_data(
                    "is_liked is required for like activities",
                ));
            }
        }
        ActivityType::Comment => {
            if payload.comment.is_none() {
                return Err(AppError::invalid_data(
                    "Comment is required for comment activities",
                ));
            }
        }
    }

    // Parse timestamp
    let timestamp = chrono::DateTime::parse_from_rfc3339(&payload.timestamp)
        .map_err(|_| AppError::invalid_data("Invalid timestamp format"))?
        .with_timezone(&Utc);

    // Create archive activity
    let archive_activity = ArchiveActivity {
        id: None,
        user_id: payload.user_id.clone(),
        username: payload.username.clone(),
        fixture_id: payload.fixture_id.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        activity_type,
        selection: payload.selection,
        is_liked: payload.is_liked,
        comment: payload.comment,
        timestamp,
        created_at: Utc::now(),
    };

    // Insert into MongoDB
    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let insert_result = collection.insert_one(archive_activity).await?;

    let activity_id = insert_result
        .inserted_id
        .as_object_id()
        .ok_or_else(|| AppError::service("Failed to get inserted ID"))?
        .to_hex();

    Ok(Json(ArchiveActivityResponse {
        success: true,
        message: "Activity archived successfully".to_string(),
        activity_id,
    }))
}

// GET /api/archive/user/:user_id
pub async fn get_user_archive(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(params): Query<ArchiveQueryParams>,
) -> Result<Json<Vec<ArchiveActivity>>> {
    println!("📁 Getting archive for user: {}", user_id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    // Build query filter
    let mut filter = doc! { "user_id": &user_id };

    // Add optional filters
    if let Some(fixture_id) = &params.fixture_id {
        filter.insert("fixture_id", fixture_id);
    }

    if let Some(activity_type) = &params.activity_type {
        filter.insert("activity_type", activity_type.to_lowercase());
    }

    // Pagination
    let page = params.page.unwrap_or(1) as i64;
    let limit = params.limit.unwrap_or(50) as i64;
    let skip = (page - 1) * limit;

    let cursor = collection
        .find(filter)
        .sort(doc! { "created_at": -1 }) // Newest first
        .skip(skip as u64)
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    println!("✅ Found {} archive activities for user: {}", activities.len(), user_id);
    Ok(Json(activities))
}

// GET /api/archive/fixture/:fixture_id
pub async fn get_fixture_archive(
    State(state): State<AppState>,
    Path(fixture_id): Path<String>,
    Query(params): Query<ArchiveQueryParams>,
) -> Result<Json<Vec<ArchiveActivity>>> {
    println!("📁 Getting archive for fixture: {}", fixture_id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let mut filter = doc! { "fixture_id": &fixture_id };

    if let Some(user_id) = &params.user_id {
        filter.insert("user_id", user_id);
    }

    if let Some(activity_type) = &params.activity_type {
        filter.insert("activity_type", activity_type.to_lowercase());
    }

    let page = params.page.unwrap_or(1) as i64;
    let limit = params.limit.unwrap_or(50) as i64;
    let skip = (page - 1) * limit;

    let cursor = collection
        .find(filter)
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    println!("✅ Found {} archive activities for fixture: {}", activities.len(), fixture_id);
    Ok(Json(activities))
}

// GET /api/archive/stats/:user_id
pub async fn get_user_archive_stats(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<UserArchiveStats>> {
    println!("📊 Getting archive stats for user: {}", user_id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    // Get total counts
    let total_votes = collection
        .count_documents(doc! { "user_id": &user_id, "activity_type": "vote" })
        .await? as i32;

    let total_likes = collection
        .count_documents(doc! { "user_id": &user_id, "activity_type": "like", "is_liked": true })
        .await? as i32;

    let total_comments = collection
        .count_documents(doc! { "user_id": &user_id, "activity_type": "comment" })
        .await? as i32;

    // Get recent activities (last 10)
    let cursor = collection
        .find(doc! { "user_id": &user_id })
        .sort(doc! { "created_at": -1 })
        .limit(10)
        .await?;

    let recent_activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    println!("✅ Stats for user {}: {} votes, {} likes, {} comments",
        user_id, total_votes, total_likes, total_comments);

    Ok(Json(UserArchiveStats {
        user_id,
        total_votes,
        total_likes,
        total_comments,
        recent_activities,
    }))
}

// GET /api/archive/search
pub async fn search_archive_activities(
    State(state): State<AppState>,
    Query(params): Query<ArchiveQueryParams>,
) -> Result<Json<Vec<ArchiveActivity>>> {
    println!("🔍 Searching archive activities");

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let mut filter = doc! {};

    if let Some(user_id) = &params.user_id {
        filter.insert("user_id", user_id);
    }

    if let Some(fixture_id) = &params.fixture_id {
        filter.insert("fixture_id", fixture_id);
    }

    if let Some(activity_type) = &params.activity_type {
        filter.insert("activity_type", activity_type.to_lowercase());
    }

    let page = params.page.unwrap_or(1) as i64;
    let limit = params.limit.unwrap_or(50) as i64;
    let skip = (page - 1) * limit;

    let cursor = collection
        .find(filter)
        .sort(doc! { "created_at": -1 })
        .skip(skip as u64)
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    println!("✅ Found {} archive activities", activities.len());
    Ok(Json(activities))
}

// DELETE /api/archive/activity/:id
pub async fn delete_archive_activity(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    println!("🗑️ Deleting archive activity: {}", id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let object_id = ObjectId::parse_str(&id)
        .map_err(|_| AppError::invalid_data("Invalid activity ID"))?;

    let result = collection.delete_one(doc! { "_id": object_id }).await?;

    if result.deleted_count == 0 {
        println!("❌ Archive activity not found: {}", id);
        return Err(AppError::DocumentNotFound);
    }

    println!("✅ Deleted archive activity: {}", id);
    Ok(StatusCode::NO_CONTENT)
}

// GET /api/archive/recent/:user_id
#[derive(Debug, Deserialize)]
pub struct RecentArchiveQuery {
    pub limit: Option<u32>,
}

pub async fn get_recent_archive_activities(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<RecentArchiveQuery>,
) -> Result<Json<Vec<ArchiveActivity>>> {
    println!("🕒 Getting recent archive activities for user: {}", user_id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let limit = query.limit.unwrap_or(20) as i64;

    let cursor = collection
        .find(doc! { "user_id": &user_id })
        .sort(doc! { "created_at": -1 })
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    println!("✅ Found {} recent archive activities for user: {}", activities.len(), user_id);
    Ok(Json(activities))
}

// GET /api/archive/check/:user_id/:fixture_id
pub async fn check_user_activity(
    State(state): State<AppState>,
    Path((user_id, fixture_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    println!("🔍 Checking user activity for {} on fixture {}", user_id, fixture_id);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    let filter = doc! {
        "user_id": &user_id,
        "fixture_id": &fixture_id
    };

    let cursor = collection.find(filter).await?;
    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;

    // Extract user's interactions
    let mut has_voted = false;
    let mut vote_selection = None;
    let mut has_liked = false;
    let mut has_commented = false;
    let mut comments = Vec::new();

    for activity in &activities {
        match &activity.activity_type {
            ActivityType::Vote => {
                has_voted = true;
                vote_selection = activity.selection.clone();
            }
            ActivityType::Like => {
                if activity.is_liked == Some(true) {
                    has_liked = true;
                }
            }
            ActivityType::Comment => {
                has_commented = true;
                if let Some(comment) = &activity.comment {
                    comments.push(comment.clone());
                }
            }
        }
    }

    let result = serde_json::json!({
        "user_id": user_id,
        "fixture_id": fixture_id,
        "has_voted": has_voted,
        "vote_selection": vote_selection,
        "has_liked": has_liked,
        "has_commented": has_commented,
        "comment_count": comments.len(),
        "comments": comments,
        "total_interactions": activities.len(),
    });

    println!("✅ User activity check complete");
    Ok(Json(result))
}

// ========== NEW METHODS ==========

// GET /api/archive/all
// GET /api/archive/all
pub async fn get_all_archives(
    State(state): State<AppState>,
    Query(query): Query<GetAllArchiveQuery>,
) -> Result<Json<PaginatedArchiveResponse>> {
    println!("📚 Getting all archive activities");
    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    // Empty filter to fetch all documents
    let filter = doc! {};

    // Pagination
    let page = query.page.unwrap_or(1) as i64;
    let limit = query.limit.unwrap_or(100) as i64;
    let skip = (page - 1) * limit;

    // Determine sort order
    let sort_order = match query.sort_by.as_deref() {
        Some("oldest") => {
            println!("📊 Sorting by: oldest first");
            1 // Ascending (oldest first)
        }
        Some("newest") | None => {
            println!("📊 Sorting by: newest first (default)");
            -1 // Descending (newest first, default)
        }
        _ => {
            return Err(AppError::invalid_data(
                "Invalid sort_by. Must be 'newest' or 'oldest'",
            ));
        }
    };

    // Get total count for pagination info
    let total_count_u64 = collection.count_documents(filter.clone()).await?;
    let total_count = total_count_u64 as i64;

    // Get paginated results
    let cursor = collection
        .find(filter.clone())
        .sort(doc! { "created_at": sort_order })
        .skip(skip as u64)
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;
    let returned_count = activities.len();

    // Get unique users count
    let pipeline = vec![
        doc! { "$group": { "_id": "$user_id" } },
        doc! { "$count": "unique_users" },
    ];

    let unique_users_cursor = collection.aggregate(pipeline).await?;
    let unique_users_result: Vec<_> = unique_users_cursor.try_collect().await?;
    let unique_users = unique_users_result
        .first()
        .and_then(|doc| doc.get_i32("unique_users").ok())
        .unwrap_or(0);

    println!(
        "✅ Found {} archive activities (page {}, limit {}), total: {}, unique users: {}",
        returned_count, page, limit, total_count, unique_users
    );

    // Calculate total pages
    let total_pages = if total_count == 0 {
        1
    } else {
        (total_count as f64 / limit as f64).ceil() as i64
    };

    Ok(Json(PaginatedArchiveResponse {
        success: true,
        data: activities,
        pagination: PaginationInfo {
            page,
            limit,
            total_items: total_count,
            total_pages,
            has_next: (page * limit) < total_count,
            has_prev: page > 1,
        },
        stats: ArchiveStats {
            unique_users,
            returned_count,
            activity_breakdown: None,
        },
        filters_applied: AppliedFilters {
            username: query.username,
            user_id: query.user_id,
            activity_type: query.activity_type,
            start_date: query.start_date,
            end_date: query.end_date,
            sort_by: query.sort_by.unwrap_or_else(|| "newest".to_string()),
        },
    }))
}

// GET /api/archive/by-username/:username
pub async fn get_archives_by_username(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Query(query): Query<GetAllArchiveQuery>,
) -> Result<Json<PaginatedArchiveResponse>> {
    println!("👤 Getting archives for username: {}", username);

    let collection: Collection<ArchiveActivity> = state.db.collection("user_archive_activities");

    // Build query filter with username
    let mut filter = doc! {
        "username": doc! {
            "$regex": format!("^{}", escape(&username)),
            "$options": "i"
        }
    };

    // Filter by activity type if provided
    if let Some(activity_type) = &query.activity_type {
        let valid_activity_type = match activity_type.to_lowercase().as_str() {
            "vote" => "vote",
            "like" => "like",
            "comment" => "comment",
            _ => {
                return Err(AppError::invalid_data(
                    "Invalid activity type. Must be 'vote', 'like', or 'comment'",
                ));
            }
        };
        filter.insert("activity_type", valid_activity_type);
    }

    // Filter by date range if provided
    if let Some(start_date) = &query.start_date {
        let start_datetime = chrono::DateTime::parse_from_rfc3339(start_date)
            .map_err(|_| AppError::invalid_data("Invalid start_date format. Use RFC3339"))?
            .with_timezone(&Utc);
        filter.insert("created_at", doc! { "$gte": start_datetime });
    }

    if let Some(end_date) = &query.end_date {
        let end_datetime = chrono::DateTime::parse_from_rfc3339(end_date)
            .map_err(|_| AppError::invalid_data("Invalid end_date format. Use RFC3339"))?
            .with_timezone(&Utc);
        filter.insert("created_at", doc! { "$lte": end_datetime });
    }

    // Pagination
    let page = query.page.unwrap_or(1) as i64;
    let limit = query.limit.unwrap_or(50) as i64;
    let skip = (page - 1) * limit;

    // Determine sort order
    let sort_order = match query.sort_by.as_deref() {
        Some("oldest") => {
            println!("📊 Sorting by: oldest first");
            1
        }
        Some("newest") | None => {
            println!("📊 Sorting by: newest first (default)");
            -1
        }
        _ => {
            return Err(AppError::invalid_data(
                "Invalid sort_by. Must be 'newest' or 'oldest'",
            ));
        }
    };

    // Get total count
    let total_count_u64 = collection.count_documents(filter.clone()).await?;
    let total_count = total_count_u64 as i64;

    // Get paginated results
    let cursor = collection
        .find(filter.clone())
        .sort(doc! { "created_at": sort_order })
        .skip(skip as u64)
        .limit(limit)
        .await?;

    let activities: Vec<ArchiveActivity> = cursor.try_collect().await?;
    let returned_count = activities.len();

    // Get activity type breakdown for this user
    let pipeline = vec![
        doc! {
            "$match": filter.clone()
        },
        doc! {
            "$group": {
                "_id": "$activity_type",
                "count": { "$sum": 1 }
            }
        },
        doc! { "$sort": { "count": -1 } }
    ];

    let breakdown_cursor = collection.aggregate(pipeline).await?;
    let breakdown_docs: Vec<_> = breakdown_cursor.try_collect().await?;

    let activity_breakdown: Vec<ActivityBreakdown> = breakdown_docs
        .into_iter()
        .filter_map(|doc| {
            Some(ActivityBreakdown {
                activity_type: doc.get_str("_id").ok()?.to_string(),
                count: doc.get_i32("count").ok()?,
            })
        })
        .collect();

    println!(
        "✅ Found {} archive activities for username: {}",
        returned_count,
        username
    );

    // Calculate total pages
    let total_pages = if total_count == 0 {
        1
    } else {
        (total_count as f64 / limit as f64).ceil() as i64
    };

    Ok(Json(PaginatedArchiveResponse {
        success: true,
        data: activities,
        pagination: PaginationInfo {
            page,
            limit,
            total_items: total_count,
            total_pages,
            has_next: (page * limit) < total_count,
            has_prev: page > 1,
        },
        stats: ArchiveStats {
            unique_users: 1, // Since we're filtering by single username
            returned_count,
            activity_breakdown: Some(activity_breakdown),
        },
        filters_applied: AppliedFilters {
            username: Some(username),
            user_id: query.user_id,
            activity_type: query.activity_type,
            start_date: query.start_date,
            end_date: query.end_date,
            sort_by: query.sort_by.unwrap_or_else(|| "newest".to_string()),
        },
    }))
}
