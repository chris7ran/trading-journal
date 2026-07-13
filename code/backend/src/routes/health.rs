//! Liveness/readiness probe.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppResult;
use crate::state::AppState;

/// `GET /health` — returns ok and verifies the DB connection is alive.
pub async fn health(State(state): State<AppState>) -> AppResult<Json<Value>> {
    // Cheap round-trip to confirm the pool is usable.
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({
        "status": "ok",
        "service": "trading-journal-api",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}
