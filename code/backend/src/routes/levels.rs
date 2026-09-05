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
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::brief::{self, Brief};
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

    let date = parse_date(query.date.as_deref())?;
    let (from, to) = levels::utc_bounds_for_query(date);
    let candles = fetch_candles(&state, &symbol, from, to).await?;

    Ok(Json(levels::compute(&symbol, date, &candles)))
}

/// `GET /brief?symbol=GER40&date=2026-08-28`
///
/// The levels plus the context that makes them mean something: average daily
/// range, where today sits against it, the daily trend, and how far the untaken
/// levels are in average-day terms.
///
/// Loads a wider slice of history than `/levels` — the averages are the point.
pub async fn get_brief(
    State(state): State<AppState>,
    Query(query): Query<LevelsQuery>,
) -> AppResult<Json<BriefResponse>> {
    let symbol = query.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err(AppError::BadRequest("symbol is required".into()));
    }

    let date = parse_date(query.date.as_deref())?;
    Ok(Json(build_brief(&state, &symbol, date).await?))
}

/// Build the full brief payload for one symbol and date.
///
/// Shared with the archive: a snapshot must freeze exactly what the screen
/// would have shown, so both paths have to go through the same code.
pub(crate) async fn build_brief(
    state: &AppState,
    symbol: &str,
    date: NaiveDate,
) -> AppResult<BriefResponse> {
    let (from, to) = levels::utc_bounds_for_history(date, brief::HISTORY_DAYS);
    let candles = fetch_candles(state, symbol, from, to).await?;

    let daily = levels::compute(symbol, date, &candles);
    let brief = brief::build(symbol, date, &candles, &daily);

    Ok(BriefResponse {
        brief,
        levels: daily,
    })
}

/// Levels and brief travel together: the app renders them on one screen, and
/// a second round trip would let the two drift apart if the feed advanced in
/// between.
#[derive(Debug, Serialize)]
pub struct BriefResponse {
    #[serde(flatten)]
    pub brief: Brief,
    pub levels: DailyLevels,
}

pub(crate) fn parse_date(raw: Option<&str>) -> AppResult<NaiveDate> {
    match raw {
        Some(value) => value.parse::<NaiveDate>().map_err(|_| {
            AppError::BadRequest(format!("invalid date '{value}', expected YYYY-MM-DD"))
        }),
        None => Ok(Utc::now().with_timezone(&DAY_TZ).date_naive()),
    }
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
pub(crate) async fn fetch_candles(
    state: &AppState,
    symbol: &str,
    from: chrono::DateTime<Utc>,
    to: chrono::DateTime<Utc>,
) -> AppResult<Vec<Candle>> {
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
