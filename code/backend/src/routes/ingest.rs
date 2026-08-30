//! Candle ingestion from the MT5 Expert Advisor.
//!
//! This is the only write path that is **not** behind the JWT. The EA is a
//! headless machine client running in a Windows VM; giving it a user session
//! would mean storing a long-lived login on a machine whose only job is to
//! forward price data. Instead it presents a dedicated `X-Ingest-Token`, which
//! grants exactly one capability: writing candles. A leak cannot read trades,
//! cannot touch accounts, and cannot authenticate as the user.
//!
//! Everything is idempotent: `(symbol, ts)` is the primary key and writes are
//! UPSERTs, so the EA may safely replay a day (or the whole backfill) after a
//! crash, a restart, or a network blip.

use axum::extract::State;
use axum::http::{HeaderMap, Request};
use axum::middleware::Next;
use axum::response::Response;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Header carrying the shared secret.
const INGEST_HEADER: &str = "x-ingest-token";

/// Upper bound on a single batch. The EA chunks its 60-day backfill.
///
/// Note this is checked *after* deserialisation, so the real memory ceiling is
/// axum's 2 MB default body limit; 2000 candles is roughly 170 KB, so the two
/// never conflict. What this constant actually buys is a clear error message
/// instead of a silent truncation.
const MAX_BATCH: usize = 2_000;

/// Longest symbol name we accept (MT5 names are short, e.g. `GER40`, `XAUUSD`).
const MAX_SYMBOL_LEN: usize = 32;

/// Tolerance for clock skew between the MT5 server and ours.
const MAX_FUTURE_SKEW_MINUTES: i64 = 30;

/// Oldest candle we accept. Anything older is a corrupt or misparsed timestamp
/// (MQL5 returns `1970-01-01` for an unset `datetime`), and storing it would
/// leave permanent junk at the far end of every range scan.
const MAX_AGE_DAYS: i64 = 3_650;

// --- Wire types -------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct IngestPayload {
    pub symbol: String,
    /// Currently only `M5` is stored; accepted and ignored so the EA can
    /// declare its intent and we can reject mismatches later without a
    /// breaking change.
    #[serde(default)]
    pub timeframe: Option<String>,
    pub candles: Vec<CandleIn>,
}

/// Deliberately terse field names: the EA hand-builds this JSON as a string,
/// and a 60-day backfill is ~66k candles. Short keys keep those payloads small.
#[derive(Debug, Deserialize)]
pub struct CandleIn {
    /// Candle OPEN time, ISO-8601, UTC.
    pub t: String,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    #[serde(default)]
    pub v: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub symbol: String,
    pub received: usize,
    pub stored: usize,
    pub first: Option<String>,
    pub last: Option<String>,
}

// --- Handlers ---------------------------------------------------------------

/// Middleware guarding every `/ingest/*` route.
///
/// This deliberately runs as a layer rather than as a check inside the handler.
/// Axum runs extractors in declaration order, so a `Json` parameter would buffer
/// and deserialise the whole body *before* the token was ever looked at — which
/// on an endpoint sitting outside the JWT means an unauthenticated caller could
/// make the server parse megabytes of JSON, and would get a 400 instead of a 401
/// for malformed input, confirming the endpoint's shape. As a layer, the body is
/// never read without a valid token.
pub async fn require_ingest_token(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    check_token(&state, req.headers())?;
    Ok(next.run(req).await)
}

/// `POST /ingest/candles` — store a batch of M5 candles.
pub async fn ingest_candles(
    State(state): State<AppState>,
    Json(payload): Json<IngestPayload>,
) -> AppResult<Json<IngestResult>> {
    let symbol = clean_symbol(&payload.symbol)?;

    if payload.candles.is_empty() {
        return Ok(Json(IngestResult {
            symbol,
            received: 0,
            stored: 0,
            first: None,
            last: None,
        }));
    }
    if payload.candles.len() > MAX_BATCH {
        return Err(AppError::BadRequest(format!(
            "batch too large: {} candles (max {MAX_BATCH})",
            payload.candles.len()
        )));
    }

    // Validate the whole batch before touching the database: a partially
    // applied batch would leave the EA unsure of where to resume.
    let now = Utc::now();
    let horizon = now + Duration::minutes(MAX_FUTURE_SKEW_MINUTES);
    let floor = now - Duration::days(MAX_AGE_DAYS);
    let mut rows: Vec<(String, f64, f64, f64, f64, Option<f64>)> =
        Vec::with_capacity(payload.candles.len());

    for (i, candle) in payload.candles.iter().enumerate() {
        let ts = parse_utc(&candle.t)
            .map_err(|e| AppError::BadRequest(format!("candle {i}: {e}")))?;
        if ts > horizon {
            return Err(AppError::BadRequest(format!(
                "candle {i}: timestamp {} is in the future — check the EA is sending UTC (TimeGMT), not server time",
                candle.t
            )));
        }
        if ts < floor {
            return Err(AppError::BadRequest(format!(
                "candle {i}: timestamp {} is implausibly old",
                candle.t
            )));
        }
        validate_ohlc(candle).map_err(|e| AppError::BadRequest(format!("candle {i}: {e}")))?;

        rows.push((
            ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            candle.o,
            candle.h,
            candle.l,
            candle.c,
            candle.v,
        ));
    }

    // Sort, then drop duplicate timestamps within the batch, so `stored`
    // reports rows actually written rather than statements executed.
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.dedup_by(|a, b| a.0 == b.0);
    let first = rows.first().map(|r| r.0.clone());
    let last = rows.last().map(|r| r.0.clone());

    // One transaction for the batch: ~40x faster than autocommit on SQLite,
    // and makes the batch all-or-nothing.
    let mut tx = state.pool.begin().await?;
    for row in &rows {
        sqlx::query(
            r#"
            INSERT INTO candles_m5 (symbol, ts, open, high, low, close, volume)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(symbol, ts) DO UPDATE SET
                open   = excluded.open,
                high   = excluded.high,
                low    = excluded.low,
                close  = excluded.close,
                volume = excluded.volume
            "#,
        )
        .bind(symbol.as_str())
        .bind(row.0.as_str())
        .bind(row.1)
        .bind(row.2)
        .bind(row.3)
        .bind(row.4)
        .bind(row.5)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    tracing::info!(symbol = %symbol, count = rows.len(), "ingested candles");

    Ok(Json(IngestResult {
        symbol,
        received: payload.candles.len(),
        stored: rows.len(),
        first,
        last,
    }))
}

/// `GET /ingest/status` — what the server already holds, per symbol.
///
/// The EA calls this on startup so it can resume from the last stored candle
/// instead of replaying the full 60-day backfill on every restart.
pub async fn ingest_status(State(state): State<AppState>) -> AppResult<Json<Value>> {
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
        .map(|(symbol, n, first, last)| {
            json!({ "symbol": symbol, "count": n, "first": first, "last": last })
        })
        .collect();

    Ok(Json(json!({ "symbols": symbols, "server_time": Utc::now().to_rfc3339() })))
}

// --- Helpers ----------------------------------------------------------------

/// Verify `X-Ingest-Token` against the configured secret.
fn check_token(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    // Trim *before* the emptiness check, not after. A whitespace-only
    // `INGEST_TOKEN` must count as "not configured": trimming afterwards would
    // leave an empty expected value that any empty header would match, turning
    // a typo in `api.env` into an open write endpoint.
    let expected = state.config.ingest_token.trim().as_bytes();
    if expected.is_empty() {
        tracing::warn!("ingest endpoint hit but INGEST_TOKEN is not configured — rejecting");
        return Err(AppError::Unauthorized);
    }

    let provided = headers
        .get(INGEST_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    // Trimming the provided value too: a trailing newline picked up from the
    // EA's input field is otherwise a permanent, silent 401 that looks exactly
    // like a wrong token.
    if constant_time_eq(provided.trim().as_bytes(), expected) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}

/// Compare two secrets without an early return on the first differing byte.
///
/// The length check does leak the secret's length, which is not considered
/// sensitive here — what matters is that an attacker cannot recover the token
/// byte by byte from response timings.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Normalise and sanity-check a symbol name.
///
/// Symbols land in log lines and are used as a grouping key, so we keep the
/// character set tight rather than trusting whatever the terminal sends.
fn clean_symbol(raw: &str) -> AppResult<String> {
    let symbol = raw.trim().to_uppercase();
    if symbol.is_empty() {
        return Err(AppError::BadRequest("symbol is required".into()));
    }
    if symbol.len() > MAX_SYMBOL_LEN {
        return Err(AppError::BadRequest(format!(
            "symbol too long (max {MAX_SYMBOL_LEN} characters)"
        )));
    }
    if !symbol
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '#'))
    {
        return Err(AppError::BadRequest(format!(
            "symbol '{symbol}' contains unsupported characters"
        )));
    }
    Ok(symbol)
}

/// Accept both `2026-08-30T07:00:00Z` and `2026-08-30 07:00:00` (the shape MT5
/// produces with `TimeToString`), always interpreting the value as UTC.
fn parse_utc(raw: &str) -> Result<DateTime<Utc>, String> {
    let trimmed = raw.trim();

    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y.%m.%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, format) {
            return Ok(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    Err(format!("unparseable timestamp '{raw}'"))
}

/// Reject candles that cannot exist, which would silently poison every level
/// computed downstream (a bogus high becomes the day's high forever).
fn validate_ohlc(c: &CandleIn) -> Result<(), String> {
    for (name, value) in [("o", c.o), ("h", c.h), ("l", c.l), ("c", c.c)] {
        if !value.is_finite() {
            return Err(format!("{name} is not a finite number"));
        }
        if value <= 0.0 {
            return Err(format!("{name} must be positive"));
        }
    }
    if c.h < c.l {
        return Err(format!("high {} is below low {}", c.h, c.l));
    }
    if c.h < c.o || c.h < c.c {
        return Err(format!("high {} does not contain open/close", c.h));
    }
    if c.l > c.o || c.l > c.c {
        return Err(format!("low {} does not contain open/close", c.l));
    }
    if let Some(v) = c.v {
        if !v.is_finite() || v < 0.0 {
            return Err("volume must be a non-negative number".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(o: f64, h: f64, l: f64, c: f64) -> CandleIn {
        CandleIn {
            t: "2026-08-30T07:00:00Z".into(),
            o,
            h,
            l,
            c,
            v: None,
        }
    }

    #[test]
    fn accepts_a_well_formed_candle() {
        assert!(validate_ohlc(&candle(100.0, 105.0, 99.0, 103.0)).is_ok());
    }

    #[test]
    fn rejects_high_below_low() {
        assert!(validate_ohlc(&candle(100.0, 98.0, 99.0, 99.0)).is_err());
    }

    #[test]
    fn rejects_high_that_does_not_contain_the_close() {
        assert!(validate_ohlc(&candle(100.0, 101.0, 99.0, 103.0)).is_err());
    }

    #[test]
    fn rejects_non_finite_prices() {
        assert!(validate_ohlc(&candle(f64::NAN, 105.0, 99.0, 103.0)).is_err());
    }

    #[test]
    fn parses_the_timestamp_shapes_mt5_can_produce() {
        let expected = "2026-08-30T07:00:00";
        for raw in [
            "2026-08-30T07:00:00Z",
            "2026-08-30 07:00:00",
            "2026.08.30 07:00:00",
        ] {
            let parsed = parse_utc(raw).expect(raw);
            assert_eq!(parsed.format("%Y-%m-%dT%H:%M:%S").to_string(), expected);
        }
    }

    #[test]
    fn treats_naive_timestamps_as_utc_not_local() {
        let parsed = parse_utc("2026-08-30 07:00:00").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-08-30T07:00:00+00:00");
    }

    #[test]
    fn normalises_symbols_and_rejects_junk() {
        assert_eq!(clean_symbol(" ger40 ").unwrap(), "GER40");
        assert!(clean_symbol("").is_err());
        assert!(clean_symbol("DROP TABLE").is_err());
        assert!(clean_symbol(&"X".repeat(64)).is_err());
    }

    #[test]
    fn constant_time_eq_still_compares_correctly() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreu"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
    }
}
