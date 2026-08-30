//! Read side of the daily-levels journal.
//!
//! Levels are computed on demand from the raw M5 candles rather than
//! precomputed and stored. The dataset is small (a couple of hundred bars per
//! symbol per day), and recomputing means a fix to the session definitions
//! applies retroactively to the whole history instead of leaving stale rows
//! behind.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::levels::{self, Candle, DailyLevels, DAY_TZ};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LevelsQuery {
    pub symbol: String,
    /// `YYYY-MM-DD`, interpreted as a Paris calendar day. Defaults to today.
    pub date: Option<String>,
}

/// `GET /levels?symbol=GER40&date=2026-08-28`
pub async fn get_levels(
    State(state): State<AppState>,
    Query(query): Query<LevelsQuery>,
) -> AppResult<Json<DailyLevels>> {
    let symbol = query.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err(AppError::BadRequest("symbol is required".into()));
    }

    let date = match query.date.as_deref() {
        Some(raw) => raw
            .parse::<NaiveDate>()
            .map_err(|_| AppError::BadRequest(format!("invalid date '{raw}', expected YYYY-MM-DD")))?,
        None => Utc::now().with_timezone(&DAY_TZ).date_naive(),
    };

    let candles = fetch_candles(&state, &symbol, date).await?;
    Ok(Json(levels::compute(&symbol, date, &candles)))
}

/// `GET /levels/symbols` — what the EA has actually delivered so far.
///
/// The mobile app uses this to populate its symbol picker, so it only ever
/// offers instruments that have data behind them.
pub async fn list_symbols(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
        r#"
        SELECT symbol, COUNT(*) AS n, MIN(ts) AS first_ts, MAX(ts) AS last_ts
        FROM candles_m5
        GROUP BY symbol
        ORDER BY symbol
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    let symbols: Vec<Value> = rows
        .into_iter()
        .map(|(symbol, count, first, last)| {
            json!({ "symbol": symbol, "count": count, "first": first, "last": last })
        })
        .collect();

    Ok(Json(json!({ "symbols": symbols })))
}

/// Load the candles needed to compute `date`, including the few prior days the
/// previous-trading-day search may reach back into.
async fn fetch_candles(
    state: &AppState,
    symbol: &str,
    date: NaiveDate,
) -> AppResult<Vec<Candle>> {
    let (from, to) = levels::utc_bounds_for_query(date);

    // Timestamps are stored in a fixed `%Y-%m-%dT%H:%M:%SZ` shape, so string
    // comparison is chronological and the primary key range-scans directly.
    let rows: Vec<(String, f64, f64, f64, f64)> = sqlx::query_as(
        r#"
        SELECT ts, open, high, low, close
        FROM candles_m5
        WHERE symbol = ?1 AND ts >= ?2 AND ts < ?3
        ORDER BY ts
        "#,
    )
    .bind(symbol)
    .bind(from.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    .bind(to.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(ts, open, high, low, close)| {
            // A row we cannot parse is a bug upstream, not a reason to fail the
            // whole request — skip it and keep the rest of the day usable.
            let ts = chrono::DateTime::parse_from_rfc3339(&ts)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| tracing::warn!(ts = %ts, error = %e, "skipping unparseable candle"))
                .ok()?;
            Some(Candle {
                ts,
                open,
                high,
                low,
                close,
            })
        })
        .collect())
}
