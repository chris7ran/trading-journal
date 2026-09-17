//! Archive of the morning brief, and the review that reads it back.
//!
//! # What this is for
//!
//! A brief is only worth keeping if you can later ask whether it helped. The
//! honest version of that question is uncomfortable: *did the brief make me
//! trade more?* Answering it needs two things stored side by side — what was
//! known before the session, and what was actually done during it.
//!
//! # Why the payload is frozen rather than recomputed
//!
//! Every figure derives from candles that are still in the database, so a brief
//! could always be rebuilt. It must not be. The rules will be refined, and a
//! recomputed brief would quietly become a different document — comparing a
//! past decision against a retroactively rewritten brief proves nothing. And a
//! brief is a moment: the one read before the London open knows yesterday and
//! the Asian range; the same brief at 22:00 also knows the day's range and its
//! close. Only the first says anything about what was known when a decision was
//! taken.
//!
//! So captures are `INSERT OR IGNORE`: the first snapshot of a (symbol, day,
//! session) wins, and no later call can overwrite it.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::levels::DAY_TZ;
use crate::routes::levels::{build_brief, parse_date};
use crate::state::AppState;

/// Pre-session captures. Instruments do not open together — the DAX at 09:00
/// Paris, the US indices at 15:30 — so one daily snapshot would be six hours
/// stale for three of the four symbols followed here.
const SESSIONS: [&str; 3] = ["pre_london", "pre_ny", "manual"];

/// Ceiling on a single capture call, so a mistyped request cannot walk the
/// whole symbol table.
const MAX_SYMBOLS_PER_CALL: usize = 20;

// --- Capture ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    /// `pre_london`, `pre_ny` or `manual`.
    pub session: String,
    /// Paris trading day. Defaults to today.
    #[serde(default)]
    pub date: Option<String>,
    /// Defaults to every symbol the feed has delivered.
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotResult {
    pub date: NaiveDate,
    pub session: String,
    pub captured_at: String,
    /// Snapshots written by this call.
    pub captured: Vec<String>,
    /// Symbols already frozen for this day and session — deliberately left alone.
    pub already_present: Vec<String>,
    /// Symbols with no usable session on that date (weekend, holiday, no data).
    pub skipped: Vec<String>,
}

/// `POST /brief/snapshot` — freeze the current brief for a whole session.
///
/// Authenticated by `X-Ingest-Token` like the candle feed: the caller is a cron
/// job, not a person, and it should not need a user session to do its work.
pub async fn create_snapshot(
    State(state): State<AppState>,
    Json(request): Json<SnapshotRequest>,
) -> AppResult<Json<SnapshotResult>> {
    let session = request.session.trim().to_lowercase();
    if !SESSIONS.contains(&session.as_str()) {
        return Err(AppError::BadRequest(format!(
            "unknown session '{session}', expected one of: {}",
            SESSIONS.join(", ")
        )));
    }

    let date = parse_date(request.date.as_deref())?;

    let symbols = match request.symbols {
        Some(list) => list
            .into_iter()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
        None => fed_symbols(&state).await?,
    };
    if symbols.is_empty() {
        return Err(AppError::BadRequest(
            "no symbol to capture — has the MT5 feed run?".into(),
        ));
    }
    if symbols.len() > MAX_SYMBOLS_PER_CALL {
        return Err(AppError::BadRequest(format!(
            "too many symbols ({}, max {MAX_SYMBOLS_PER_CALL})",
            symbols.len()
        )));
    }

    let captured_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut result = SnapshotResult {
        date,
        session: session.clone(),
        captured_at: captured_at.clone(),
        captured: Vec::new(),
        already_present: Vec::new(),
        skipped: Vec::new(),
    };

    for symbol in symbols {
        let brief = build_brief(&state, &symbol, date).await?;

        // A day with no session at all carries no information; archiving it
        // would only pad the review with empty rows.
        if brief.levels.day.is_none() && brief.brief.bias.previous_mid.is_none() {
            result.skipped.push(symbol);
            continue;
        }

        let payload = serde_json::to_string(&brief)
            .map_err(|e| AppError::Other(anyhow::anyhow!("serialising brief: {e}")))?;

        let rows = sqlx::query(
            r#"
            INSERT OR IGNORE INTO brief_snapshots (
                id, symbol, date, session, captured_at, payload,
                bias_read, open_above_mid, previous_mid, adr,
                day_pct_of_adr, asia_pct_of_median
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(symbol.as_str())
        .bind(date.to_string())
        .bind(session.as_str())
        .bind(captured_at.as_str())
        .bind(payload)
        .bind(serde_json::to_value(brief.brief.bias.read).ok().and_then(|v| v.as_str().map(str::to_string)))
        .bind(brief.brief.bias.open_above_mid.map(i64::from))
        .bind(brief.brief.bias.previous_mid)
        .bind(brief.brief.stats.adr)
        .bind(brief.brief.stats.day_pct_of_adr)
        .bind(brief.brief.stats.asia_pct_of_median)
        .execute(&state.pool)
        .await?
        .rows_affected();

        if rows == 0 {
            result.already_present.push(symbol);
        } else {
            result.captured.push(symbol);
        }
    }

    tracing::info!(
        session = %result.session,
        date = %result.date,
        captured = result.captured.len(),
        "brief snapshots"
    );
    Ok(Json(result))
}

// --- Read back --------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SnapshotSummary {
    pub symbol: String,
    pub date: String,
    pub session: String,
    pub captured_at: String,
    pub bias_read: Option<String>,
    pub open_above_mid: Option<bool>,
    pub previous_mid: Option<f64>,
    pub adr: Option<f64>,
    pub day_pct_of_adr: Option<f64>,
    pub asia_pct_of_median: Option<f64>,
}

type SummaryRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

fn to_summary(row: SummaryRow) -> SnapshotSummary {
    SnapshotSummary {
        symbol: row.0,
        date: row.1,
        session: row.2,
        captured_at: row.3,
        bias_read: row.4,
        open_above_mid: row.5.map(|v| v != 0),
        previous_mid: row.6,
        adr: row.7,
        day_pct_of_adr: row.8,
        asia_pct_of_median: row.9,
    }
}

const SUMMARY_COLUMNS: &str = "symbol, date, session, captured_at, bias_read, \
     open_above_mid, previous_mid, adr, day_pct_of_adr, asia_pct_of_median";

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub symbol: Option<String>,
    pub limit: Option<i64>,
}

/// `GET /brief/history?symbol=GER40&limit=60` — what has been archived.
pub async fn history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> AppResult<Json<Vec<SnapshotSummary>>> {
    let limit = query.limit.unwrap_or(60).clamp(1, 500);
    let symbol = query.symbol.map(|s| s.trim().to_uppercase());

    let sql = format!(
        "SELECT {SUMMARY_COLUMNS} FROM brief_snapshots
         WHERE (?1 IS NULL OR symbol = ?1)
         ORDER BY date DESC, session, symbol
         LIMIT ?2"
    );
    let rows: Vec<SummaryRow> = sqlx::query_as(&sql)
        .bind(symbol)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(rows.into_iter().map(to_summary).collect()))
}

#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    pub symbol: String,
    pub date: String,
    pub session: Option<String>,
}

/// `GET /brief/archived?symbol=GER40&date=2026-09-04&session=pre_london`
///
/// Returns the frozen payload verbatim — not a fresh computation. Kept on a
/// distinct path from the `POST /brief/snapshot` that writes it, because the
/// two carry different credentials: a person reads, a cron writes.
pub async fn get_snapshot(
    State(state): State<AppState>,
    Query(query): Query<SnapshotQuery>,
) -> AppResult<Json<Value>> {
    let symbol = query.symbol.trim().to_uppercase();
    let session = query.session.unwrap_or_else(|| "pre_london".into());

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT payload, captured_at FROM brief_snapshots
         WHERE symbol = ?1 AND date = ?2 AND session = ?3",
    )
    .bind(&symbol)
    .bind(&query.date)
    .bind(&session)
    .fetch_optional(&state.pool)
    .await?;

    let (payload, captured_at) = row.ok_or(AppError::NotFound)?;
    let brief: Value = serde_json::from_str(&payload)
        .map_err(|e| AppError::Other(anyhow::anyhow!("stored payload is not valid JSON: {e}")))?;

    Ok(Json(json!({
        "session": session,
        "captured_at": captured_at,
        "archived": true,
        "brief": brief,
    })))
}

// --- Review -----------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct TradeTally {
    pub count: i64,
    pub pnl: f64,
}

#[derive(Debug, Serialize)]
pub struct ReviewDay {
    pub date: String,
    pub snapshots: Vec<SnapshotSummary>,
    /// Trades on the same instrument that day.
    pub trades_symbol: TradeTally,
    /// Every trade that day, whatever the instrument — the figure that answers
    /// "am I trading more since the brief exists?".
    pub trades_all: TradeTally,
}

#[derive(Debug, Deserialize)]
pub struct ReviewQuery {
    pub symbol: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// `GET /brief/review?symbol=GER40&from=2026-09-01&to=2026-09-30`
///
/// The brief that was frozen before each session, next to the trades actually
/// taken that day. No verdict is computed: with a handful of weeks of history,
/// any average by bias reading would be noise dressed up as insight. That comes
/// later, once each reading has enough occurrences to mean something.
pub async fn review(
    State(state): State<AppState>,
    Query(query): Query<ReviewQuery>,
) -> AppResult<Json<Vec<ReviewDay>>> {
    let symbol = query.symbol.map(|s| s.trim().to_uppercase());
    let today = Utc::now().with_timezone(&DAY_TZ).date_naive();
    let to = match query.to.as_deref() {
        Some(raw) => parse_date(Some(raw))?,
        None => today,
    };
    let from = match query.from.as_deref() {
        Some(raw) => parse_date(Some(raw))?,
        None => to - chrono::Duration::days(30),
    };
    if from > to {
        return Err(AppError::BadRequest("'from' is after 'to'".into()));
    }

    let sql = format!(
        "SELECT {SUMMARY_COLUMNS} FROM brief_snapshots
         WHERE date >= ?1 AND date <= ?2 AND (?3 IS NULL OR symbol = ?3)
         ORDER BY date DESC, session, symbol"
    );
    let rows: Vec<SummaryRow> = sqlx::query_as(&sql)
        .bind(from.to_string())
        .bind(to.to_string())
        .bind(symbol.clone())
        .fetch_all(&state.pool)
        .await?;

    // Trades are grouped on the date part of `open_time`, which the MT5 import
    // stores as the broker's own local time. Converting it would mean guessing
    // the broker's offset for a past date; taking the date as written matches
    // how the journal reads on screen. A position opened either side of
    // midnight can land on the neighbouring day — rare, and preferable to a
    // conversion that is confidently wrong.
    let trade_rows: Vec<(String, String, Option<f64>)> = sqlx::query_as(
        "SELECT substr(open_time, 1, 10) AS day, symbol, pnl
         FROM trades
         WHERE open_time IS NOT NULL
           AND substr(open_time, 1, 10) >= ?1
           AND substr(open_time, 1, 10) <= ?2",
    )
    .bind(from.to_string())
    .bind(to.to_string())
    .fetch_all(&state.pool)
    .await?;

    let mut days: Vec<ReviewDay> = Vec::new();
    let mut day_for = |days: &mut Vec<ReviewDay>, date: &str| -> usize {
        if let Some(i) = days.iter().position(|d| d.date == date) {
            return i;
        }
        days.push(ReviewDay {
            date: date.to_string(),
            snapshots: Vec::new(),
            trades_symbol: TradeTally { count: 0, pnl: 0.0 },
            trades_all: TradeTally { count: 0, pnl: 0.0 },
        });
        days.len() - 1
    };

    for row in rows {
        let summary = to_summary(row);
        let i = day_for(&mut days, &summary.date);
        days[i].snapshots.push(summary);
    }

    // Days with trades but no snapshot are kept on purpose: they are the
    // "before the brief existed" baseline, and dropping them would flatter the
    // comparison by only ever showing days the brief covered.
    for (trade_day, trade_symbol, pnl) in &trade_rows {
        let i = day_for(&mut days, trade_day);
        let value = pnl.unwrap_or(0.0);
        days[i].trades_all.count += 1;
        days[i].trades_all.pnl += value;

        // Only meaningful when the review is scoped to one instrument;
        // otherwise "the symbol" has no single answer for the day.
        if let Some(wanted) = symbol.as_deref() {
            if wanted.eq_ignore_ascii_case(trade_symbol) {
                days[i].trades_symbol.count += 1;
                days[i].trades_symbol.pnl += value;
            }
        }
    }

    days.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(Json(days))
}

// --- Helpers ----------------------------------------------------------------

/// Symbols the MT5 feed has actually delivered.
async fn fed_symbols(state: &AppState) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT symbol FROM candles_m5 ORDER BY symbol")
            .fetch_all(&state.pool)
            .await?;
    Ok(rows.into_iter().map(|(s,)| s).collect())
}
