//! `POST /trades/import/csv` — bulk import from an MT5 history export.
//!
//! Request: raw CSV or .xlsx in the request body.
//! `?account_id=...` targets an explicit account. When omitted, the account is
//! resolved from the report's `Compte:` number (created on first sight); failing
//! that, the seeded default account is used.
//! Rows with a known `mt5_ticket` are refreshed in place (pnl/fees) rather than
//! duplicated.

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::csv_import::mt5::{self, ParseResult};
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
    /// The account the trades landed in (resolved or freshly created).
    pub account_id: String,
    pub account_name: String,
}

pub async fn import_csv(
    State(state): State<AppState>,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> AppResult<Json<ImportSummary>> {
    if body.is_empty() {
        return Err(AppError::BadRequest("empty request body".into()));
    }

    // .xlsx files are ZIP archives starting with "PK\x03\x04". Anything else is
    // treated as CSV text. This lets the app upload the raw MT5 export directly.
    let parsed = if body.starts_with(b"PK\x03\x04") {
        mt5::parse_xlsx(&body).map_err(AppError::BadRequest)?
    } else {
        // MT5 sometimes exports UTF-16/Latin-1; decode leniently.
        let csv_text = String::from_utf8_lossy(&body);
        if csv_text.trim().is_empty() {
            return Err(AppError::BadRequest("empty request body".into()));
        }
        mt5::parse(&csv_text).map_err(AppError::BadRequest)?
    };

    let (account_id, account_name) = resolve_account(&state, &query, &parsed).await?;

    let mut summary = ImportSummary {
        imported: 0,
        skipped_duplicates: 0,
        failed: 0,
        warnings: parsed.warnings,
        account_id: account_id.clone(),
        account_name,
    };

    for trade in parsed.trades {
        // A known ticket is refreshed (fees may have changed), not re-inserted.
        let existed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM trades WHERE mt5_ticket IS NOT NULL AND mt5_ticket = ?)",
        )
        .bind(&trade.mt5_ticket)
        .fetch_one(&state.pool)
        .await?;

        let id = uuid::Uuid::new_v4().to_string();
        let res = sqlx::query(
            r#"
            INSERT INTO trades (
                id, account_id, symbol, direction, open_time, close_time,
                open_price, close_price, lot_size, pnl, pnl_pct, commission, swap,
                setup_tag, emotion_tag, notes, screenshot_url, mt5_ticket
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(mt5_ticket) DO UPDATE SET
                pnl        = excluded.pnl,
                commission = excluded.commission,
                swap       = excluded.swap
            "#,
        )
        .bind(&id)
        .bind(&account_id)
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
            Ok(_) if existed => summary.skipped_duplicates += 1,
            Ok(_) => summary.imported += 1,
            Err(e) => {
                summary.failed += 1;
                tracing::warn!("trade insert failed during import: {e}");
                summary
                    .warnings
                    .push("une ligne n'a pas pu être importée".into());
            }
        }
    }

    Ok(Json(summary))
}

/// Decide which account the import targets, returning `(id, name)`.
/// Priority: explicit `?account_id` → the report's `Compte:` number (created on
/// demand) → the seeded default account.
async fn resolve_account(
    state: &AppState,
    query: &ImportQuery,
    parsed: &ParseResult,
) -> AppResult<(String, String)> {
    if let Some(account_id) = &query.account_id {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT id, name FROM accounts WHERE id = ?")
                .bind(account_id)
                .fetch_optional(&state.pool)
                .await?;
        return row
            .ok_or_else(|| AppError::BadRequest(format!("account '{account_id}' does not exist")));
    }

    if let Some(number) = &parsed.account_number {
        // Existing MT5 accounts are keyed by their login number in `name`.
        let existing: Option<(String, String)> =
            sqlx::query_as("SELECT id, name FROM accounts WHERE name = ?")
                .bind(number)
                .fetch_optional(&state.pool)
                .await?;
        if let Some(row) = existing {
            return Ok(row);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let broker = parsed.account_name.as_deref().unwrap_or("MT5");
        sqlx::query(
            r#"
            INSERT INTO accounts (id, name, broker, balance, currency, is_funded)
            VALUES (?, ?, ?, 100000, 'USD', 1)
            "#,
        )
        .bind(&id)
        .bind(number)
        .bind(broker)
        .execute(&state.pool)
        .await?;
        return Ok((id, number.clone()));
    }

    // No hint at all: fall back to the seeded default account.
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, name FROM accounts WHERE id = ?")
            .bind(DEFAULT_ACCOUNT)
            .fetch_optional(&state.pool)
            .await?;
    row.ok_or_else(|| {
        AppError::BadRequest("no account_id given and no default account exists".into())
    })
}
