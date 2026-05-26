use axum::{
    extract::{Query, State},
    response::Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId},
    Collection,
};
use serde::Deserialize;

use crate::{
    errors::{AppError, Result},
    models::pledges::{CreatePledge, Pledge, PledgeQuery},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct PledgeStatsQuery {
    pub home_team: Option<String>,
    pub away_team: Option<String>,
}

// Get all pledges with optional filtering
pub async fn get_pledges(
    State(state): State<AppState>,
    Query(query): Query<PledgeQuery>,
) -> Result<Json<Vec<Pledge>>> {
    println!("🔍 GET /api/pledges called - Starting MongoDB query...");

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let mut filter = doc! {};

    if let Some(username) = &query.username {
        filter.insert("username", username);
    }

    if let Some(phone) = &query.phone {
        filter.insert("phone", phone);
    }

    if let Some(home_team) = &query.home_team {
        filter.insert("home_team", home_team);
    }

    if let Some(away_team) = &query.away_team {
        filter.insert("away_team", away_team);
    }

    let cursor = collection.find(filter).await?;
    let mut pledges: Vec<Pledge> = cursor.try_collect().await?;

    pledges.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("✅ Successfully fetched {} pledges", pledges.len());
    Ok(Json(pledges))
}

// Create a new pledge
pub async fn create_pledge(
    State(state): State<AppState>,
    Json(payload): Json<CreatePledge>,
) -> Result<Json<Pledge>> {
    println!("🎯 Creating new pledge for user: {}", payload.username);

    // Validate required fields — each gets its own error message
    if payload.username.is_empty() {
        return Err(AppError::MissingRequiredField("username".to_string()));
    }
    if payload.phone.is_empty() {
        return Err(AppError::MissingRequiredField("phone".to_string()));
    }
    if payload.selection.is_empty() {
        return Err(AppError::MissingRequiredField("selection".to_string()));
    }

    if payload.amount <= 0.0 {
        return Err(AppError::ValidationError(
            "amount must be greater than 0".to_string(),
        ));
    }

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let pledge = Pledge {
        _id: Some(ObjectId::new()),
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        selection: payload.selection.clone(),
        amount: payload.amount,
        time: Utc::now(),
        fan: payload.fan.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        starter_id: payload.starter_id.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    collection.insert_one(&pledge).await?;

    println!(
        "✅ Successfully created pledge for user: {} - Amount: ₿{}",
        payload.username, payload.amount
    );
    Ok(Json(pledge))
}

// Get pledge statistics for a specific match
pub async fn get_pledge_stats(
    State(state): State<AppState>,
    Query(query): Query<PledgeStatsQuery>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting pledge statistics...");

    // Both home_team and away_team are required
    let home_team = query
        .home_team
        .as_ref()
        .ok_or_else(|| AppError::MissingRequiredField("home_team".to_string()))?;
    let away_team = query
        .away_team
        .as_ref()
        .ok_or_else(|| AppError::MissingRequiredField("away_team".to_string()))?;

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let filter = doc! {
        "home_team": home_team,
        "away_team": away_team
    };

    let cursor = collection.find(filter.clone()).await?;
    let pledges: Vec<Pledge> = cursor.try_collect().await?;

    let total_pledges = pledges.len() as i64;
    let total_amount: f64 = pledges.iter().map(|p| p.amount).sum();

    let home_pledges = pledges
        .iter()
        .filter(|p| p.selection == "home_team")
        .count() as i64;
    let away_pledges = pledges
        .iter()
        .filter(|p| p.selection == "away_team")
        .count() as i64;
    let draw_pledges = pledges.iter().filter(|p| p.selection == "draw").count() as i64;

    let stats = serde_json::json!({
        "total_pledges": total_pledges,
        "total_amount": total_amount,
        "selection_breakdown": {
            "home_team": home_pledges,
            "away_team": away_pledges,
            "draw": draw_pledges
        },
        "match": {
            "home_team": home_team,
            "away_team": away_team
        }
    });

    println!("✅ Successfully fetched pledge statistics");
    Ok(Json(stats))
}

// Get user's pledging history
pub async fn get_user_pledges(
    State(state): State<AppState>,
    Query(query): Query<PledgeQuery>,
) -> Result<Json<Vec<Pledge>>> {
    println!("👤 Getting user pledges...");

    let username = query
        .username
        .ok_or_else(|| AppError::MissingRequiredField("username".to_string()))?;

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let filter = doc! { "username": &username };
    let cursor = collection.find(filter).await?;
    let mut pledges: Vec<Pledge> = cursor.try_collect().await?;

    pledges.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("✅ Successfully fetched {} pledges for user", pledges.len());
    Ok(Json(pledges))
}

pub async fn get_recent_pledges(State(state): State<AppState>) -> Result<Json<Vec<Pledge>>> {
    println!("🕒 Getting recent pledges...");

    let collection: Collection<Pledge> = state.db.collection("pledges");

    let cursor = collection.find(doc! {}).await?;
    let mut pledges: Vec<Pledge> = cursor.try_collect().await?;

    pledges.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let recent_pledges: Vec<Pledge> = pledges.into_iter().take(10).collect();

    println!(
        "✅ Successfully fetched {} recent pledges",
        recent_pledges.len()
    );
    Ok(Json(recent_pledges))
}
