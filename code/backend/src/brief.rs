//! Contextual statistics for the morning brief.
//!
//! # Why this module exists at all
//!
//! Raw levels answer "where". They do not answer "is this move already big?".
//! Sixty-two points from the previous day's high means nothing on its own —
//! it is trivial on a day the instrument routinely travels 300 points, and a
//! long way on a day it travels 90. Every figure here is therefore expressed
//! against the instrument's **own recent behaviour**, chiefly the Average Daily
//! Range, so the same number carries the same meaning on GER40 and on XAUUSD.
//!
//! # What this module deliberately does not do
//!
//! It never recommends. Every sentence it produces is a measurement: "the day
//! has travelled 168% of its average range". Whether that is a reason to act,
//! to wait, or to ignore is the reader's call. A tool that says "volatility is
//! high, watch for opportunities" has added nothing except false confidence,
//! and a brief read 250 times a year is exactly where false confidence
//! compounds. The same discipline is what makes these numbers safe to feed to
//! a language model later: it will be given facts to phrase, never figures to
//! compute.

use chrono::{Duration, NaiveDate};
use serde::Serialize;

use crate::levels::{self, Candle, DailyLevels, Ohlc, DAY_TZ};

/// Sessions used for every average and median. Twenty is the usual window for
/// an ADR: long enough to be stable, short enough to track a regime change.
pub const LOOKBACK_SESSIONS: usize = 20;

/// Calendar days of candles a caller must load to compute a brief.
pub const HISTORY_DAYS: i64 = 45;

/// Below this many completed sessions the averages are not worth showing.
const MIN_SESSIONS_FOR_STATS: usize = 5;

// --- Types ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RangeStats {
    /// Completed sessions actually used (may be fewer than `LOOKBACK_SESSIONS`).
    pub sessions: usize,
    /// Average daily range over the lookback, in price units.
    pub adr: Option<f64>,
    /// Median daily range — less swayed by one violent session than the mean.
    pub adr_median: Option<f64>,
    pub day_range: Option<f64>,
    /// Today's range as a percentage of the ADR. 100 = an average day.
    pub day_pct_of_adr: Option<f64>,
    pub previous_day_range: Option<f64>,
    pub previous_day_pct_of_adr: Option<f64>,
    pub asia_range: Option<f64>,
    pub asia_median: Option<f64>,
    pub asia_pct_of_median: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Up,
    Down,
    Range,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrendRead {
    pub direction: TrendDirection,
    pub sessions: usize,
    pub sma: Option<f64>,
    pub last_close: Option<f64>,
    /// Distance from the moving average, as a percentage of the ADR.
    pub close_vs_sma_pct_of_adr: Option<f64>,
    /// Net move over the lookback, in price units.
    pub net_move: Option<f64>,
    /// That net move measured in average days — the figure the direction rests on.
    pub net_move_in_adr: Option<f64>,
    pub higher_highs_5: usize,
    pub lower_lows_5: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelDistance {
    pub key: String,
    pub label: String,
    pub price: f64,
    /// Signed: positive when the level sits above the reference price.
    pub distance: f64,
    pub pct_of_adr: Option<f64>,
    pub above: bool,
    pub swept: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub key: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Brief {
    pub symbol: String,
    pub date: NaiveDate,
    /// Last close available — what distances are measured from.
    pub reference_price: Option<f64>,
    pub stats: RangeStats,
    pub trend: TrendRead,
    pub distances: Vec<LevelDistance>,
    pub observations: Vec<Observation>,
}

// --- Entry point ------------------------------------------------------------

/// Build the brief for `symbol` on `date`.
///
/// `candles` must be sorted ascending and cover [`HISTORY_DAYS`] before the
/// date, so the averages rest on real history rather than on three sessions.
pub fn build(symbol: &str, date: NaiveDate, candles: &[Candle], levels: &DailyLevels) -> Brief {
    // Averages are computed on **completed** sessions only. Including today
    // would mean comparing a half-formed range against the average that
    // contains it, and every morning would look artificially quiet.
    let history = levels::daily_series(candles, date - Duration::days(1), LOOKBACK_SESSIONS);

    let stats = range_stats(candles, &history, levels);
    let trend = trend_read(&history, stats.adr);

    let reference_price = levels
        .day
        .as_ref()
        .map(|d| d.close)
        .or_else(|| history.last().map(|(_, o)| o.close));

    let distances = distances(levels, reference_price, stats.adr);
    let observations = observations(&stats, &trend, levels, &distances);

    Brief {
        symbol: symbol.to_string(),
        date,
        reference_price,
        stats,
        trend,
        distances,
        observations,
    }
}

// --- Range statistics -------------------------------------------------------

fn range_stats(
    candles: &[Candle],
    history: &[(NaiveDate, Ohlc)],
    levels: &DailyLevels,
) -> RangeStats {
    let ranges: Vec<f64> = history.iter().map(|(_, o)| o.range).collect();
    let enough = ranges.len() >= MIN_SESSIONS_FOR_STATS;

    let adr = if enough { mean(&ranges) } else { None };
    let adr_median = if enough { median(&ranges) } else { None };

    let day_range = levels.day.as_ref().map(|d| d.range);
    let previous_day_range = levels.previous_day.as_ref().map(|p| p.ohlc.range);

    // The Asian range for each of the same sessions, rebuilt from its real
    // timezone window rather than a fixed UTC slice.
    let asia_history: Vec<f64> = history
        .iter()
        .filter_map(|(day, _)| {
            let (start, end) = levels::window_bounds("asia_range", *day)?;
            levels::aggregate_window(candles, start, end).map(|o| o.range)
        })
        .collect();

    let asia_range = levels
        .sessions
        .iter()
        .find(|w| w.key == "asia_range")
        .and_then(|w| w.ohlc.as_ref())
        .map(|o| o.range);
    let asia_median = if asia_history.len() >= MIN_SESSIONS_FOR_STATS {
        median(&asia_history)
    } else {
        None
    };

    RangeStats {
        sessions: ranges.len(),
        adr,
        adr_median,
        day_range,
        day_pct_of_adr: ratio_pct(day_range, adr),
        previous_day_range,
        previous_day_pct_of_adr: ratio_pct(previous_day_range, adr),
        asia_range,
        asia_median,
        asia_pct_of_median: ratio_pct(asia_range, asia_median),
    }
}

// --- Trend ------------------------------------------------------------------

/// Read the daily trend from completed sessions.
///
/// The direction test is deliberately scale-free: the net move over the window
/// is compared to **one average day's range**. If twenty sessions have produced
/// less than a single average day of net travel, that is a range regardless of
/// what the closes did in between — and the same rule reads correctly on a
/// 26,000-point index and on a 4,400-point metal, which a percentage threshold
/// would not.
fn trend_read(history: &[(NaiveDate, Ohlc)], adr: Option<f64>) -> TrendRead {
    let sessions = history.len();
    if sessions < MIN_SESSIONS_FOR_STATS {
        return TrendRead {
            direction: TrendDirection::Unknown,
            sessions,
            sma: None,
            last_close: None,
            close_vs_sma_pct_of_adr: None,
            net_move: None,
            net_move_in_adr: None,
            higher_highs_5: 0,
            lower_lows_5: 0,
        };
    }

    let closes: Vec<f64> = history.iter().map(|(_, o)| o.close).collect();
    let sma = mean(&closes);
    let last_close = *closes.last().expect("checked non-empty");
    let first_close = closes[0];
    let net_move = last_close - first_close;

    let net_move_in_adr = adr.filter(|a| *a > 0.0).map(|a| net_move / a);
    let direction = match (net_move_in_adr, sma) {
        (Some(moved), Some(average)) => {
            if moved.abs() < 1.0 {
                TrendDirection::Range
            } else if moved > 0.0 && last_close > average {
                TrendDirection::Up
            } else if moved < 0.0 && last_close < average {
                TrendDirection::Down
            } else {
                // Net move and position around the mean disagree — a turn in
                // progress. Calling that a trend would be overreaching.
                TrendDirection::Range
            }
        }
        _ => TrendDirection::Unknown,
    };

    // Structure over the last five sessions, counted pairwise.
    let tail = &history[history.len().saturating_sub(6)..];
    let mut higher_highs = 0;
    let mut lower_lows = 0;
    for pair in tail.windows(2) {
        if pair[1].1.high > pair[0].1.high {
            higher_highs += 1;
        }
        if pair[1].1.low < pair[0].1.low {
            lower_lows += 1;
        }
    }

    TrendRead {
        direction,
        sessions,
        sma,
        last_close: Some(last_close),
        close_vs_sma_pct_of_adr: match (sma, adr) {
            (Some(average), Some(a)) if a > 0.0 => Some((last_close - average) / a * 100.0),
            _ => None,
        },
        net_move: Some(net_move),
        net_move_in_adr,
        higher_highs_5: higher_highs,
        lower_lows_5: lower_lows,
    }
}

// --- Distances --------------------------------------------------------------

fn distances(
    levels: &DailyLevels,
    reference: Option<f64>,
    adr: Option<f64>,
) -> Vec<LevelDistance> {
    let Some(price) = reference else {
        return Vec::new();
    };

    let mut out: Vec<LevelDistance> = Vec::new();
    let mut push = |key: String, label: String, level: Option<f64>| {
        let Some(level) = level else { return };
        if !level.is_finite() {
            return;
        }
        let distance = level - price;
        out.push(LevelDistance {
            swept: swept_at(levels, level).is_some(),
            key,
            label,
            price: level,
            distance,
            pct_of_adr: adr
                .filter(|a| *a > 0.0)
                .map(|a| distance.abs() / a * 100.0),
            above: distance >= 0.0,
        });
    };

    if let Some(prev) = &levels.previous_day {
        push("pdh".into(), "PDH".into(), Some(prev.ohlc.high));
        push("pdl".into(), "PDL".into(), Some(prev.ohlc.low));
    }
    for window in &levels.opening_ranges {
        push(
            format!("{}_high", window.key),
            format!("{} haut", short_label(&window.key, &window.label)),
            window.ohlc.as_ref().map(|o| o.high),
        );
        push(
            format!("{}_low", window.key),
            format!("{} bas", short_label(&window.key, &window.label)),
            window.ohlc.as_ref().map(|o| o.low),
        );
    }
    for window in &levels.sessions {
        if window.key != "asia_range" {
            continue; // the one session range worth measuring against pre-open
        }
        push(
            "asia_high".into(),
            "Haut du range asiatique".into(),
            window.ohlc.as_ref().map(|o| o.high),
        );
        push(
            "asia_low".into(),
            "Bas du range asiatique".into(),
            window.ohlc.as_ref().map(|o| o.low),
        );
    }

    // Nearest first: what price has to get through before anything else.
    out.sort_by(|a, b| {
        a.distance
            .abs()
            .partial_cmp(&b.distance.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn short_label(key: &str, fallback: &str) -> String {
    match key {
        "orb_london" => "ORB Londres".to_string(),
        "orb_newyork" => "ORB New York".to_string(),
        _ => fallback.to_string(),
    }
}

/// Paris-time `HH:MM` of the sweep that fired on this level, if any.
fn swept_at(levels: &DailyLevels, level: f64) -> Option<String> {
    let tolerance = levels
        .day
        .as_ref()
        .map(|d| (d.range / 10_000.0).max(1e-6))
        .unwrap_or(1e-6);

    levels
        .sweeps
        .iter()
        .find(|s| s.swept && (s.level - level).abs() <= tolerance)
        .and_then(|s| s.at)
        .map(|at| at.with_timezone(&DAY_TZ).format("%H:%M").to_string())
}

// --- Observations -----------------------------------------------------------

/// Plain measurements, in French. No verb of recommendation appears here.
fn observations(
    stats: &RangeStats,
    trend: &TrendRead,
    levels: &DailyLevels,
    distances: &[LevelDistance],
) -> Vec<Observation> {
    let mut out = Vec::new();

    if let (Some(range), Some(pct), Some(adr)) =
        (stats.day_range, stats.day_pct_of_adr, stats.adr)
    {
        out.push(Observation {
            key: "day_range",
            text: format!(
                "Amplitude du jour : {} pts, soit {} % de l'ADR {} séances ({} pts).",
                pts(range),
                pct.round(),
                stats.sessions,
                pts(adr)
            ),
        });
    }

    if let (Some(range), Some(pct)) = (stats.previous_day_range, stats.previous_day_pct_of_adr) {
        let date = levels
            .previous_day
            .as_ref()
            .map(|p| p.date.format("%d/%m").to_string())
            .unwrap_or_else(|| "la veille".into());
        out.push(Observation {
            key: "previous_day_range",
            text: format!(
                "Séance précédente ({date}) : {} pts parcourus, {} % de l'ADR.",
                pts(range),
                pct.round()
            ),
        });
    }

    if let (Some(range), Some(pct), Some(median)) =
        (stats.asia_range, stats.asia_pct_of_median, stats.asia_median)
    {
        out.push(Observation {
            key: "asia_range",
            text: format!(
                "Range asiatique : {} pts, {} % de sa médiane 20 séances ({} pts).",
                pts(range),
                pct.round(),
                pts(median)
            ),
        });
    }

    if trend.direction != TrendDirection::Unknown {
        let label = match trend.direction {
            TrendDirection::Up => "haussière",
            TrendDirection::Down => "baissière",
            _ => "sans direction nette",
        };
        let detail = match trend.net_move_in_adr {
            Some(moved) => format!(
                " Sur {} séances, le net parcouru vaut {:.1} journée(s) moyenne(s).",
                trend.sessions,
                moved.abs()
            ),
            None => String::new(),
        };
        out.push(Observation {
            key: "trend",
            text: format!("Tendance daily : {label}.{detail}"),
        });
    }

    if trend.higher_highs_5 > 0 || trend.lower_lows_5 > 0 {
        out.push(Observation {
            key: "structure",
            text: format!(
                "Sur les 5 dernières séances : {} plus haut(s) supérieur(s) à la veille, {} plus bas inférieur(s).",
                trend.higher_highs_5, trend.lower_lows_5
            ),
        });
    }

    // The two nearest untouched levels — what stands in the way, and how far
    // that is in average-day terms.
    for level in distances.iter().filter(|d| !d.swept).take(2) {
        let direction = if level.above { "au-dessus" } else { "en dessous" };
        let in_adr = match level.pct_of_adr {
            Some(p) => format!(", soit {} % d'ADR", p.round()),
            None => String::new(),
        };
        out.push(Observation {
            key: "distance",
            text: format!(
                "{} non pris : {} {} de la référence ({} pts{in_adr}).",
                level.label,
                pts(level.price),
                direction,
                pts(level.distance.abs())
            ),
        });
    }

    for sweep in levels.sweeps.iter().filter(|s| s.swept) {
        if let Some(at) = sweep.at {
            out.push(Observation {
                key: "sweep",
                text: format!(
                    "{} à {} ({} pts au-delà).",
                    sweep.label,
                    at.with_timezone(&DAY_TZ).format("%H:%M"),
                    pts(sweep.excursion)
                ),
            });
        }
    }

    out
}

// --- Small maths ------------------------------------------------------------

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    })
}

/// `value / reference * 100`, guarding against a zero or missing reference.
fn ratio_pct(value: Option<f64>, reference: Option<f64>) -> Option<f64> {
    match (value, reference) {
        (Some(v), Some(r)) if r > 0.0 => Some(v / r * 100.0),
        _ => None,
    }
}

/// Price units, one decimal — enough for index points, readable in a sentence.
fn pts(value: f64) -> String {
    format!("{:.1}", value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    /// A session of `bars` M5 candles on `day`, spanning `high`..`low`.
    /// Starts at 08:00 UTC so it lands squarely inside the Paris calendar day.
    fn session(day: NaiveDate, bars: usize, high: f64, low: f64) -> Vec<Candle> {
        let start = utc(&format!("{day}T08:00:00Z"));
        (0..bars)
            .map(|i| Candle {
                ts: start + Duration::minutes(5 * i as i64),
                open: low,
                high: if i == 0 { high } else { low },
                low,
                close: low,
            })
            .collect()
    }

    #[test]
    fn mean_and_median_behave() {
        assert_eq!(mean(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(mean(&[]), None);
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn median_resists_a_single_violent_session() {
        // One 500-point day among nineteen 100-point days.
        let mut ranges = vec![100.0; 19];
        ranges.push(500.0);
        let m = mean(&ranges).unwrap();
        let med = median(&ranges).unwrap();
        assert!(m > 115.0, "the mean is dragged up: {m}");
        assert_eq!(med, 100.0, "the median is not");
    }

    #[test]
    fn ratio_pct_guards_against_a_zero_reference() {
        assert_eq!(ratio_pct(Some(150.0), Some(100.0)), Some(150.0));
        assert_eq!(ratio_pct(Some(150.0), Some(0.0)), None);
        assert_eq!(ratio_pct(None, Some(100.0)), None);
    }

    fn history_from(ranges: &[(f64, f64, f64)]) -> Vec<(NaiveDate, Ohlc)> {
        // (high, low, close) per session, oldest first, on consecutive dates.
        ranges
            .iter()
            .enumerate()
            .map(|(i, (high, low, close))| {
                let day = date("2026-06-01") + Duration::days(i as i64);
                (
                    day,
                    Ohlc {
                        open: *low,
                        high: *high,
                        low: *low,
                        close: *close,
                        range: high - low,
                        bars: 200,
                        high_at: utc("2026-06-01T10:00:00Z"),
                        low_at: utc("2026-06-01T11:00:00Z"),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_market_going_nowhere_is_called_a_range_not_a_trend() {
        // Ten sessions, 100-point days, closing where it started.
        let mut rows = Vec::new();
        for i in 0..10 {
            let base = 1000.0 + if i % 2 == 0 { 10.0 } else { -10.0 };
            rows.push((base + 50.0, base - 50.0, base));
        }
        let history = history_from(&rows);
        let trend = trend_read(&history, Some(100.0));
        assert_eq!(trend.direction, TrendDirection::Range);
    }

    #[test]
    fn a_sustained_advance_is_called_up() {
        // Ten sessions climbing 40 points each: 360 net on a 100-point ADR.
        let rows: Vec<(f64, f64, f64)> = (0..10)
            .map(|i| {
                let base = 1000.0 + 40.0 * i as f64;
                (base + 50.0, base - 50.0, base)
            })
            .collect();
        let history = history_from(&rows);
        let trend = trend_read(&history, Some(100.0));

        assert_eq!(trend.direction, TrendDirection::Up);
        assert_eq!(trend.net_move, Some(360.0));
        assert_eq!(trend.net_move_in_adr, Some(3.6));
        assert_eq!(trend.higher_highs_5, 5);
        assert_eq!(trend.lower_lows_5, 0);
    }

    #[test]
    fn the_direction_test_is_scale_free() {
        // Same shape at two very different price scales must read the same.
        let small: Vec<(f64, f64, f64)> = (0..10)
            .map(|i| {
                let b = 4000.0 + 40.0 * i as f64;
                (b + 50.0, b - 50.0, b)
            })
            .collect();
        let large: Vec<(f64, f64, f64)> = (0..10)
            .map(|i| {
                let b = 26000.0 + 40.0 * i as f64;
                (b + 50.0, b - 50.0, b)
            })
            .collect();

        assert_eq!(
            trend_read(&history_from(&small), Some(100.0)).direction,
            trend_read(&history_from(&large), Some(100.0)).direction,
        );
    }

    #[test]
    fn too_little_history_is_reported_as_unknown_rather_than_guessed() {
        let history = history_from(&[(1050.0, 950.0, 1000.0), (1060.0, 960.0, 1010.0)]);
        let trend = trend_read(&history, Some(100.0));
        assert_eq!(trend.direction, TrendDirection::Unknown);
        assert!(trend.sma.is_none());
    }

    /// The whole point of computing the ADR on `date - 1` and back: a violent
    /// day in progress must not inflate the average it is being compared to.
    #[test]
    fn adr_ignores_the_session_currently_in_progress() {
        let today = date("2026-07-30");
        let mut candles = Vec::new();

        // Ten quiet 100-point sessions before today.
        for back in 1..=10 {
            candles.extend(session(today - Duration::days(back), 60, 1100.0, 1000.0));
        }
        // Today: a 500-point day.
        candles.extend(session(today, 60, 1500.0, 1000.0));
        candles.sort_by_key(|c| c.ts);

        let history = levels::daily_series(&candles, today - Duration::days(1), LOOKBACK_SESSIONS);
        let ranges: Vec<f64> = history.iter().map(|(_, o)| o.range).collect();

        assert_eq!(ranges.len(), 10);
        assert!(
            ranges.iter().all(|r| (*r - 100.0).abs() < 1e-6),
            "today's 500-point range must not appear: {ranges:?}"
        );
        assert_eq!(mean(&ranges), Some(100.0));

        // And the day therefore reads as 500% of the ADR, not 145%.
        assert_eq!(ratio_pct(Some(500.0), mean(&ranges)), Some(500.0));
    }
}
