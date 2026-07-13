//! Handlers for `/setups` — named trading setups / patterns with their rules
//! and structured fields (description, target entry/exit, stop loss).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{NewSetup, Setup, UpdateSetup};
use crate::state::AppState;

/// `GET /setups` — list setups (alphabetical).
pub async fn list_setups(State(state): State<AppState>) -> AppResult<Json<Vec<Setup>>> {
    let setups = sqlx::query_as::<_, Setup>("SELECT * FROM setups ORDER BY name")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(setups))
}

/// `POST /setups` — create a setup.
pub async fn create_setup(
    State(state): State<AppState>,
    Json(input): Json<NewSetup>,
) -> AppResult<(StatusCode, Json<Setup>)> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO setups (id, name, rules, description, target_entry, target_exit, stop_loss)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(input.name.trim())
    .bind(&input.rules)
    .bind(&input.description)
    .bind(&input.target_entry)
    .bind(&input.target_exit)
    .bind(&input.stop_loss)
    .execute(&state.pool)
    .await?;

    let setup = sqlx::query_as::<_, Setup>("SELECT * FROM setups WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;
    Ok((StatusCode::CREATED, Json(setup)))
}

/// `PUT /setups/:id` — update a setup. Omitted fields keep their current value.
pub async fn update_setup(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSetup>,
) -> AppResult<Json<Setup>> {
    let name = input.name.map(|s| s.trim().to_string());

    sqlx::query(
        "UPDATE setups SET
            name         = COALESCE(?, name),
            rules        = COALESCE(?, rules),
            description  = COALESCE(?, description),
            target_entry = COALESCE(?, target_entry),
            target_exit  = COALESCE(?, target_exit),
            stop_loss    = COALESCE(?, stop_loss)
         WHERE id = ?",
    )
    .bind(&name)
    .bind(&input.rules)
    .bind(&input.description)
    .bind(&input.target_entry)
    .bind(&input.target_exit)
    .bind(&input.stop_loss)
    .bind(&id)
    .execute(&state.pool)
    .await?;

    let setup = sqlx::query_as::<_, Setup>("SELECT * FROM setups WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::BadRequest("setup not found".into()))?;
    Ok(Json(setup))
}
