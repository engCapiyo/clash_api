use axum::{
    extract::{Query, State},
    response::Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId},
    ClientSession, Collection,
};
use serde::Deserialize;

use crate::{
    errors::{AppError, Result},
    models::pledges::{CreatePledge, Pledge, PledgeQuery},
    models::user::User,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct PledgeStatsQuery {
    pub home_team: Option<String>,
    pub away_team: Option<String>,
}

// ============================================================================
// GET ALL PLEDGES WITH OPTIONAL FILTERING
// ============================================================================

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
    if let Some(starter_id) = &query.starter_id {
        filter.insert("starter_id", starter_id);
    }
    if let Some(fixture_id) = &query.fixture_id {
        filter.insert("fixture_id", fixture_id);
    }

    let cursor = collection.find(filter).await?;
    let mut pledges: Vec<Pledge> = cursor.try_collect().await?;

    pledges.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("✅ Successfully fetched {} pledges", pledges.len());
    Ok(Json(pledges))
}

// ============================================================================
// CREATE PLEDGE WITH TRANSACTION & BALANCE DEDUCTION
// ============================================================================

pub async fn create_pledge(
    State(state): State<AppState>,
    Json(payload): Json<CreatePledge>,
) -> Result<Json<Pledge>> {
    println!(
        "💰 Creating pledge for user: {} (amount: KES {:.2})",
        payload.username, payload.amount
    );

    // ============================================================
    // 1️⃣ VALIDATE INPUT
    // ============================================================
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
    if payload.starter_id.is_empty() {
        return Err(AppError::MissingRequiredField("starter_id".to_string()));
    }

    let users_col: Collection<User> = state.db.collection("users");
    let pledges_col: Collection<Pledge> = state.db.collection("pledges");

    // ============================================================
    // 2️⃣ START SESSION & TRANSACTION
    //
    // NOTE: mongodb 3.x requires a `Client` to start a session —
    // `Database` does not expose `start_session()`. This means
    // `AppState` must hold a `mongodb::Client` (e.g. `state.client`)
    // alongside `db`. See the accompanying note on `AppState`.
    // ============================================================
    let mut session: ClientSession = state.client.start_session().await?;
    session.start_transaction().await?;

    // ============================================================
    // 3️⃣ FIND USER BY starter_id (within the transaction)
    //
    // In 3.x there is no `find_one_with_session`. Instead, chain
    // `.session(&mut session)` onto the normal `find_one` call.
    // ============================================================
    let user_result = users_col
        .find_one(doc! { "user_id": &payload.starter_id })
        .session(&mut session)
        .await;

    let user = match user_result {
        Ok(Some(u)) => u,
        Ok(None) => {
            session.abort_transaction().await?;
            return Err(AppError::DocumentNotFound);
        }
        Err(e) => {
            session.abort_transaction().await?;
            return Err(e.into());
        }
    };

    // ============================================================
    // 4️⃣ CHECK SUFFICIENT BALANCE
    // ============================================================
    if user.balance < payload.amount {
        session.abort_transaction().await?;
        return Err(AppError::ValidationError(format!(
            "Insufficient balance. You have KES {:.2}, need KES {:.2}",
            user.balance, payload.amount
        )));
    }

    // ============================================================
    // 5️⃣ DEDUCT BALANCE (WITHIN TRANSACTION)
    // ============================================================
    let now = Utc::now();

    let update_result = users_col
        .update_one(
            doc! { "user_id": &payload.starter_id },
            doc! {
                "$inc": { "balance": -payload.amount },
                "$set": { "updated_at": now }
            },
        )
        .session(&mut session)
        .await;

    if let Err(e) = update_result {
        session.abort_transaction().await?;
        return Err(e.into());
    }

    // ============================================================
    // 6️⃣ CREATE PLEDGE RECORD (WITHIN TRANSACTION)
    // ============================================================
    let pledge = Pledge {
        _id: Some(ObjectId::new()),
        username: payload.username.clone(),
        phone: payload.phone.clone(),
        selection: payload.selection.clone(),
        amount: payload.amount,
        time: now,
        fan: payload.fan.clone(),
        home_team: payload.home_team.clone(),
        away_team: payload.away_team.clone(),
        starter_id: payload.starter_id.clone(),
        fixture_id: payload.fixture_id.clone(),
        created_at: now,
        updated_at: now,
    };

    let insert_result = pledges_col.insert_one(&pledge).session(&mut session).await;

    if let Err(e) = insert_result {
        session.abort_transaction().await?;
        return Err(e.into());
    }

    // ============================================================
    // 7️⃣ COMMIT TRANSACTION
    // ============================================================
    session.commit_transaction().await?;

    println!(
        "✅ Pledge created: {} - Amount: KES {:.2} - New balance: KES {:.2}",
        payload.username,
        payload.amount,
        user.balance - payload.amount
    );

    Ok(Json(pledge))
}

// ============================================================================
// GET PLEDGE STATISTICS FOR A SPECIFIC MATCH
// ============================================================================

pub async fn get_pledge_stats(
    State(state): State<AppState>,
    Query(query): Query<PledgeStatsQuery>,
) -> Result<Json<serde_json::Value>> {
    println!("📊 Getting pledge statistics...");

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

    let cursor = collection.find(filter).await?;
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

// ============================================================================
// GET USER'S PLEDGING HISTORY
// ============================================================================

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

// ============================================================================
// GET RECENT PLEDGES (FOR SOCIAL PROOF)
// ============================================================================

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
