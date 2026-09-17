//! Chart candles: the raw M5 feed folded up to a chart timeframe.
//!
//! The levels screen draws its price ladder *on* a chart, and lines without
//! bars behind them say nothing — so this endpoint exists purely to give the
//! app something to draw them over.
//!
//! No new storage: `candles_m5` is the single source of truth, and higher
//! timeframes are folded on read. An M5 day is a couple of hundred rows, so the
//! aggregation costs nothing measurable, and a fix to the feed shows up on
//! every timeframe at once instead of leaving stale derived tables behind.

use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::levels::{self, Candle};
use crate::routes::levels::{fetch_candles, parse_date};
use crate::state::AppState;

/// Calendar days of history returned when the caller does not ask.
///
/// Two weeks is about eight or nine sessions: enough for the mobile chart to
/// offer a 5-day view without a second round trip, small enough to stay a few
/// hundred bars on the wire.
const DEFAULT_DAYS: i64 = 14;

/// Hard ceiling, so a stray `days=100000` cannot ask SQLite for the universe.
const MAX_DAYS: i64 = 120;

/// Supported chart timeframes, as `(name, minutes)`.
///
/// `D1` is deliberately absent. A daily bar is a *Paris* day whose boundary
/// moves with DST, which is [`crate::levels`]' business — folding it here on
/// UTC hour boundaries would quietly produce a different daily candle from the
/// one the levels are computed against.
const TIMEFRAMES: &[(&str, i64)] = &[("M5", 5), ("M15", 15), ("M30", 30), ("H1", 60), ("H4", 240)];

#[derive(Debug, Deserialize)]
pub struct CandlesQuery {
    pub symbol: String,
    /// `YYYY-MM-DD`, interpreted as a Paris calendar day. Defaults to today.
    pub date: Option<String>,
    /// One of [`TIMEFRAMES`]. Defaults to `H1`.
    pub tf: Option<String>,
    /// Calendar days of history *before* `date`. Defaults to [`DEFAULT_DAYS`].
    pub days: Option<i64>,
}

/// One aggregated bar.
#[derive(Debug, Clone, Serialize)]
pub struct Bar {
    pub ts: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// M5 candles folded into this bar — a partial bar says so rather than
    /// pretending to be complete.
    pub bars: usize,
}

#[derive(Debug, Serialize)]
pub struct CandlesResponse {
    pub symbol: String,
    pub timeframe: String,
    pub candles: Vec<Bar>,
}

/// `GET /candles?symbol=GER40&date=2026-09-17&tf=H1&days=14`
pub async fn get_candles(
    State(state): State<AppState>,
    Query(query): Query<CandlesQuery>,
) -> AppResult<Json<CandlesResponse>> {
    let symbol = query.symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err(AppError::BadRequest("symbol is required".into()));
    }

    let (name, minutes) = parse_timeframe(query.tf.as_deref())?;
    let date = parse_date(query.date.as_deref())?;
    let days = query.days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);

    // The window stops at the end of `date` rather than running on: asking for
    // a past day and getting the next morning's bars drawn to the right of the
    // close would misread as part of that session.
    let (day_start, day_end) = levels::utc_day_bounds(date);
    let from = day_start - Duration::days(days);

    let m5 = fetch_candles(&state, &symbol, from, day_end).await?;

    Ok(Json(CandlesResponse {
        symbol,
        timeframe: name.to_string(),
        candles: aggregate(&m5, minutes),
    }))
}

fn parse_timeframe(raw: Option<&str>) -> AppResult<(&'static str, i64)> {
    let wanted = raw.unwrap_or("H1").trim().to_uppercase();
    TIMEFRAMES
        .iter()
        .find(|(name, _)| *name == wanted)
        .copied()
        .ok_or_else(|| {
            let supported: Vec<&str> = TIMEFRAMES.iter().map(|(name, _)| *name).collect();
            AppError::BadRequest(format!(
                "unknown timeframe '{wanted}', expected one of {}",
                supported.join(", ")
            ))
        })
}

/// Fold ascending M5 candles into `minutes`-wide bars.
///
/// Buckets are anchored on the epoch, so every bar starts on a round clock
/// time. Every supported timeframe divides an hour or is a whole number of
/// hours, and every exchange offset in play is a whole number of hours, so
/// "round in UTC" and "round in local time" are the same boundary.
///
/// A timeframe with no candles in it yields no bar at all. The alternative —
/// emitting a flat placeholder — would draw a doji across a market holiday.
fn aggregate(candles: &[Candle], minutes: i64) -> Vec<Bar> {
    let width = minutes * 60;
    let mut out: Vec<Bar> = Vec::new();

    for c in candles {
        let slot = c.ts.timestamp().div_euclid(width) * width;

        // The input is ordered by timestamp, so only the bar being built can
        // still match — no lookup needed.
        match out.last_mut() {
            Some(bar) if bar.ts.timestamp() == slot => {
                bar.high = bar.high.max(c.high);
                bar.low = bar.low.min(c.low);
                bar.close = c.close;
                bar.bars += 1;
            }
            _ => out.push(Bar {
                ts: Utc.timestamp_opt(slot, 0).single().unwrap_or(c.ts),
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                bars: 1,
            }),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candle(ts: &str, open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle {
            ts: DateTime::parse_from_rfc3339(ts)
                .unwrap()
                .with_timezone(&Utc),
            open,
            high,
            low,
            close,
        }
    }

    #[test]
    fn folds_an_hour_of_m5_into_one_bar() {
        let m5 = vec![
            candle("2026-09-17T09:00:00Z", 100.0, 103.0, 99.0, 102.0),
            candle("2026-09-17T09:05:00Z", 102.0, 108.0, 101.0, 107.0),
            candle("2026-09-17T09:55:00Z", 107.0, 107.5, 95.0, 96.0),
        ];

        let bars = aggregate(&m5, 60);

        assert_eq!(bars.len(), 1);
        let bar = &bars[0];
        assert_eq!(bar.ts.to_rfc3339(), "2026-09-17T09:00:00+00:00");
        assert_eq!(bar.open, 100.0); // first candle's open
        assert_eq!(bar.close, 96.0); // last candle's close
        assert_eq!(bar.high, 108.0);
        assert_eq!(bar.low, 95.0);
        assert_eq!(bar.bars, 3);
    }

    #[test]
    fn a_new_hour_starts_a_new_bar() {
        let m5 = vec![
            candle("2026-09-17T09:55:00Z", 100.0, 101.0, 99.0, 100.5),
            candle("2026-09-17T10:00:00Z", 100.5, 104.0, 100.0, 103.0),
        ];

        let bars = aggregate(&m5, 60);

        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 100.5);
        assert_eq!(bars[1].open, 100.5);
        assert_eq!(bars[1].high, 104.0);
    }

    #[test]
    fn a_missing_hour_leaves_a_hole_rather_than_a_flat_bar() {
        let m5 = vec![
            candle("2026-09-17T09:00:00Z", 100.0, 101.0, 99.0, 100.0),
            // 10:00 never traded — a holiday half-day, or a feed gap.
            candle("2026-09-17T11:00:00Z", 100.0, 102.0, 98.0, 101.0),
        ];

        let bars = aggregate(&m5, 60);

        assert_eq!(bars.len(), 2);
        assert_eq!(bars[1].ts.to_rfc3339(), "2026-09-17T11:00:00+00:00");
    }

    #[test]
    fn h4_buckets_land_on_round_hours() {
        let m5 = vec![
            candle("2026-09-17T07:55:00Z", 100.0, 101.0, 99.0, 100.0),
            candle("2026-09-17T08:00:00Z", 100.0, 105.0, 100.0, 104.0),
            candle("2026-09-17T11:55:00Z", 104.0, 106.0, 103.0, 105.0),
            candle("2026-09-17T12:00:00Z", 105.0, 107.0, 104.0, 106.0),
        ];

        let bars = aggregate(&m5, 240);

        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].ts.to_rfc3339(), "2026-09-17T04:00:00+00:00");
        assert_eq!(bars[1].ts.to_rfc3339(), "2026-09-17T08:00:00+00:00");
        assert_eq!(bars[1].bars, 2);
        assert_eq!(bars[2].ts.to_rfc3339(), "2026-09-17T12:00:00+00:00");
    }

    #[test]
    fn no_candles_no_bars() {
        assert!(aggregate(&[], 60).is_empty());
    }

    #[test]
    fn unknown_timeframe_is_rejected_and_h1_is_the_default() {
        assert_eq!(parse_timeframe(None).unwrap(), ("H1", 60));
        assert_eq!(parse_timeframe(Some("h4")).unwrap(), ("H4", 240));
        assert!(parse_timeframe(Some("D1")).is_err());
        assert!(parse_timeframe(Some("nonsense")).is_err());
    }
}
