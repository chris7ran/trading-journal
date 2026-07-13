//! `POST /trades/import/csv` — bulk import from an MT5 history CSV export.
//!
//! Request: raw CSV in the request body (Content-Type: text/csv).
//! Optional query param `?account_id=...` (defaults to the seeded account).
//! Deduplication: rows with an `mt5_ticket` that already exists are skipped.

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::csv_import::mt5;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const DEFAULT_ACCOUNT: &str = "default";

#[derive(Debug, Deserialize)]
pub struct ImportQuery {
    pub account_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub failed: usize,
    pub warnings: Vec<String>,
}

pub async fn import_csv(
    State(state): State<AppState>,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> AppResult<Json<ImportSummary>> {
    let account_id = query
        .account_id
        .unwrap_or_else(|| DEFAULT_ACCOUNT.to_string());

    // Verify the target account exists for a clear error instead of an FK failure.
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM accounts WHERE id = ?")
        .bind(&account_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::BadRequest(format!(
            "account '{account_id}' does not exist"
        )));
    }

    if body.is_empty() {
        return Err(AppError::BadRequest("empty request body".into()));
    }

    // .xlsx files are ZIP archives starting with "PK\x03\x04". Anything else is
    // treated as CSV text. This lets the app upload the raw MT5 export directly.
    let parsed = if body.starts_with(b"PK\x03\x04") {
        mt5::parse_xlsx(&body, &account_id).map_err(AppError::BadRequest)?
    } else {
        // MT5 sometimes exports UTF-16/Latin-1; decode leniently.
        let csv_text = String::from_utf8_lossy(&body);
        if csv_text.trim().is_empty() {
            return Err(AppError::BadRequest("empty request body".into()));
        }
        mt5::parse(&csv_text, &account_id).map_err(AppError::BadRequest)?
    };

    let mut summary = ImportSummary {
        imported: 0,
        skipped_duplicates: 0,
        failed: 0,
        warnings: parsed.warnings,
    };

    // Insert each trade in its own statement, ignoring duplicate tickets.
    // ON CONFLICT(mt5_ticket) DO NOTHING => rows_affected == 0 means a dupe.
    for trade in parsed.trades {
        let id = uuid::Uuid::new_v4().to_string();
        let res = sqlx::query(
            r#"
            INSERT INTO trades (
                id, account_id, symbol, direction, open_time, close_time,
                open_price, close_price, lot_size, pnl, pnl_pct, commission, swap,
                setup_tag, emotion_tag, notes, screenshot_url, mt5_ticket
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(mt5_ticket) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(&trade.account_id)
        .bind(&trade.symbol)
        .bind(&trade.direction)
        .bind(&trade.open_time)
        .bind(&trade.close_time)
        .bind(trade.open_price)
        .bind(trade.close_price)
        .bind(trade.lot_size)
        .bind(trade.pnl)
        .bind(trade.pnl_pct)
        .bind(trade.commission)
        .bind(trade.swap)
        .bind(&trade.setup_tag)
        .bind(&trade.emotion_tag)
        .bind(&trade.notes)
        .bind(&trade.screenshot_url)
        .bind(&trade.mt5_ticket)
        .execute(&state.pool)
        .await;

        match res {
            Ok(r) if r.rows_affected() == 0 => summary.skipped_duplicates += 1,
            Ok(_) => summary.imported += 1,
            Err(e) => {
                summary.failed += 1;
                summary.warnings.push(format!("insert failed: {e}"));
            }
        }
    }

    Ok(Json(summary))
}
