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

use chrono::{Datelike, Duration, NaiveDate};
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

/// Descriptive reading of the session against the previous day's midpoint.
///
/// These are *observations of what happened*, not forecasts. In particular,
/// `Continuation*` and `Reversal*` can only be established once the session has
/// traded — see the `known_at_open` flag on [`DailyBias`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BiasRead {
    /// Opened above the midpoint and the low never came back below it.
    ContinuationHaussiere,
    /// Opened below and the high never came back above.
    ContinuationBaissiere,
    /// Took the previous high, then closed back under the midpoint.
    ReversalDepuisLeHaut,
    /// Took the previous low, then closed back above the midpoint.
    ReversalDepuisLeBas,
    /// The midpoint was traded through both ways — no clean signature.
    ZeroCinqTraverse,
    Indetermine,
}

/// Daily bias built on the previous session's midpoint.
///
/// # The one distinction that matters here
///
/// Only `open_above_mid` is knowable **before** the session — it is the actual
/// bias. `held_above_mid`, `closed_above_mid` and the reversal flags describe a
/// session that has already traded, and are recorded for the review, not for a
/// decision. Measuring them on a completed day and calling the result a
/// prediction is circular: a day whose low never returned below the midpoint is
/// by construction a strongly directional up-day, so of course it reached the
/// previous high. That number looks impressive and forecasts nothing.
#[derive(Debug, Clone, Serialize)]
pub struct DailyBias {
    /// Midpoint of the previous session's range — the reference level.
    pub previous_mid: Option<f64>,
    pub previous_high: Option<f64>,
    pub previous_low: Option<f64>,

    /// The only forward-looking fact in this struct.
    pub open_above_mid: Option<bool>,
    /// True while nothing else here is usable ahead of the session.
    pub known_at_open: bool,

    /// Session low stayed above the midpoint — continuation intact.
    pub held_above_mid: Option<bool>,
    /// Session high stayed below the midpoint.
    pub held_below_mid: Option<bool>,
    pub closed_above_mid: Option<bool>,

    pub touched_pdh: Option<bool>,
    pub touched_pdl: Option<bool>,
    /// Which extreme was reached first, inferred from the timestamps of the
    /// session's high and low. An approximation, but it separates a directional
    /// day from one that swept both sides.
    pub first_taken: Option<&'static str>,

    /// Reversal signature: took the previous high, then closed under the midpoint.
    pub reversal_from_high: Option<bool>,
    pub reversal_from_low: Option<bool>,

    pub read: BiasRead,
}

/// Where the week stands: which session made its high, which made its low.
///
/// This is the input a weekly-profile framework needs — whether the extreme is
/// already in, and on which weekday it printed.
#[derive(Debug, Clone, Serialize)]
pub struct WeeklyProfile {
    pub week_start: NaiveDate,
    /// Completed sessions in the week up to and including `date`.
    pub sessions: usize,
    pub today_weekday: &'static str,
    pub high: Option<f64>,
    pub high_day: Option<NaiveDate>,
    pub high_weekday: Option<&'static str>,
    pub low: Option<f64>,
    pub low_day: Option<NaiveDate>,
    pub low_weekday: Option<&'static str>,
    pub range: Option<f64>,
    /// Weekly range as a percentage of the ADR — is the week already wide?
    pub range_pct_of_adr: Option<f64>,
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
    pub bias: DailyBias,
    pub weekly: WeeklyProfile,
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
    let bias = daily_bias(levels);
    let weekly = weekly_profile(candles, date, stats.adr);

    let reference_price = levels
        .day
        .as_ref()
        .map(|d| d.close)
        .or_else(|| history.last().map(|(_, o)| o.close));

    let distances = distances(levels, reference_price, stats.adr, bias.previous_mid);
    let observations = observations(&stats, &bias, &weekly, levels, &distances);

    Brief {
        symbol: symbol.to_string(),
        date,
        reference_price,
        stats,
        bias,
        weekly,
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

// --- Daily bias -------------------------------------------------------------

/// Read the session against the previous session's midpoint.
///
/// The midpoint is `(previous high + previous low) / 2` — the middle of the
/// range, not of the body. Every flag below is a plain measurement; the
/// `known_at_open` field marks how much of it was available before the session
/// started, which is the difference between a bias and a description.
fn daily_bias(levels: &DailyLevels) -> DailyBias {
    let empty = DailyBias {
        previous_mid: None,
        previous_high: None,
        previous_low: None,
        open_above_mid: None,
        known_at_open: false,
        held_above_mid: None,
        held_below_mid: None,
        closed_above_mid: None,
        touched_pdh: None,
        touched_pdl: None,
        first_taken: None,
        reversal_from_high: None,
        reversal_from_low: None,
        read: BiasRead::Indetermine,
    };

    let Some(previous) = levels.previous_day.as_ref() else {
        return empty;
    };
    let (pdh, pdl) = (previous.ohlc.high, previous.ohlc.low);
    if !(pdh > pdl) {
        return empty;
    }
    let mid = (pdh + pdl) / 2.0;

    // The midpoint exists as soon as yesterday closed, even if today has not
    // traded yet — that is precisely the pre-session case worth serving.
    let Some(day) = levels.day.as_ref() else {
        return DailyBias {
            previous_mid: Some(mid),
            previous_high: Some(pdh),
            previous_low: Some(pdl),
            known_at_open: true,
            ..empty
        };
    };

    let touched_pdh = day.high > pdh;
    let touched_pdl = day.low < pdl;

    // Which extreme printed first, from the timestamps of the session's high
    // and low. Only meaningful when both sides were reached.
    let first_taken = match (touched_pdh, touched_pdl) {
        (true, true) => Some(if day.high_at < day.low_at { "PDH" } else { "PDL" }),
        (true, false) => Some("PDH"),
        (false, true) => Some("PDL"),
        (false, false) => None,
    };

    let held_above = day.low > mid;
    let held_below = day.high < mid;
    let closed_above = day.close > mid;

    let reversal_from_high = touched_pdh && day.close < mid;
    let reversal_from_low = touched_pdl && day.close > mid;

    // Reversal takes precedence: taking the previous extreme and closing back
    // through the midpoint is the stronger signature, and it can coexist with
    // an open on the "wrong" side of the level.
    let read = if reversal_from_high {
        BiasRead::ReversalDepuisLeHaut
    } else if reversal_from_low {
        BiasRead::ReversalDepuisLeBas
    } else if held_above {
        BiasRead::ContinuationHaussiere
    } else if held_below {
        BiasRead::ContinuationBaissiere
    } else {
        BiasRead::ZeroCinqTraverse
    };

    DailyBias {
        previous_mid: Some(mid),
        previous_high: Some(pdh),
        previous_low: Some(pdl),
        open_above_mid: Some(day.open > mid),
        known_at_open: true,
        held_above_mid: Some(held_above),
        held_below_mid: Some(held_below),
        closed_above_mid: Some(closed_above),
        touched_pdh: Some(touched_pdh),
        touched_pdl: Some(touched_pdl),
        first_taken,
        reversal_from_high: Some(reversal_from_high),
        reversal_from_low: Some(reversal_from_low),
        read,
    }
}

// --- Weekly profile ---------------------------------------------------------

const WEEKDAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];

fn weekday_name(day: NaiveDate) -> &'static str {
    WEEKDAYS[day.weekday().num_days_from_monday() as usize]
}

/// Which session made the week's high, which made its low, and how wide the
/// week already is.
///
/// Weekly-profile frameworks hinge on whether an extreme is already in and on
/// which weekday it printed, so both the value and the day are reported.
fn weekly_profile(candles: &[Candle], date: NaiveDate, adr: Option<f64>) -> WeeklyProfile {
    let week_start = date - Duration::days(date.weekday().num_days_from_monday() as i64);

    let mut sessions = 0usize;
    let mut high: Option<(f64, NaiveDate)> = None;
    let mut low: Option<(f64, NaiveDate)> = None;

    let mut day = week_start;
    while day <= date {
        if let Some((_, ohlc)) = levels::daily_series(candles, day, 1)
            .into_iter()
            .find(|(d, _)| *d == day)
        {
            sessions += 1;
            if high.map_or(true, |(h, _)| ohlc.high > h) {
                high = Some((ohlc.high, day));
            }
            if low.map_or(true, |(l, _)| ohlc.low < l) {
                low = Some((ohlc.low, day));
            }
        }
        day += Duration::days(1);
    }

    let range = match (high, low) {
        (Some((h, _)), Some((l, _))) => Some(h - l),
        _ => None,
    };

    WeeklyProfile {
        week_start,
        sessions,
        today_weekday: weekday_name(date),
        high: high.map(|(v, _)| v),
        high_day: high.map(|(_, d)| d),
        high_weekday: high.map(|(_, d)| weekday_name(d)),
        low: low.map(|(v, _)| v),
        low_day: low.map(|(_, d)| d),
        low_weekday: low.map(|(_, d)| weekday_name(d)),
        range,
        range_pct_of_adr: ratio_pct(range, adr),
    }
}

// --- Distances --------------------------------------------------------------

fn distances(
    levels: &DailyLevels,
    reference: Option<f64>,
    adr: Option<f64>,
    previous_mid: Option<f64>,
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
    push(
        "previous_mid".into(),
        "0,5 de la veille".into(),
        previous_mid,
    );
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
    bias: &DailyBias,
    weekly: &WeeklyProfile,
    levels: &DailyLevels,
    distances: &[LevelDistance],
) -> Vec<Observation> {
    let mut out = Vec::new();

    // --- The bias first: it is what the session is read against. ------------

    if let Some(mid) = bias.previous_mid {
        let mut text = format!("0,5 de la veille : {}.", pts(mid));
        if let Some(above) = bias.open_above_mid {
            text.push_str(if above {
                " Ouverture au-dessus."
            } else {
                " Ouverture en dessous."
            });
        }
        out.push(Observation {
            key: "bias_mid",
            text,
        });
    }

    match bias.read {
        BiasRead::ContinuationHaussiere => out.push(Observation {
            key: "bias_read",
            text: "Continuation haussière : le bas de la séance n'est jamais repassé sous le 0,5.".into(),
        }),
        BiasRead::ContinuationBaissiere => out.push(Observation {
            key: "bias_read",
            text: "Continuation baissière : le haut de la séance n'est jamais repassé au-dessus du 0,5.".into(),
        }),
        BiasRead::ReversalDepuisLeHaut => out.push(Observation {
            key: "bias_read",
            text: "Reversal depuis le haut : le PDH a été pris, puis clôture sous le 0,5.".into(),
        }),
        BiasRead::ReversalDepuisLeBas => out.push(Observation {
            key: "bias_read",
            text: "Reversal depuis le bas : le PDL a été pris, puis clôture au-dessus du 0,5.".into(),
        }),
        BiasRead::ZeroCinqTraverse => out.push(Observation {
            key: "bias_read",
            text: "Le 0,5 de la veille a été traversé dans les deux sens : pas de signature nette.".into(),
        }),
        BiasRead::Indetermine => {}
    }

    if let Some(first) = bias.first_taken {
        out.push(Observation {
            key: "bias_first",
            text: format!("Premier extrême de la veille atteint : {first}."),
        });
    }

    // --- The guardrail: is the move already made? ---------------------------

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

    // --- Where the week stands ----------------------------------------------

    if let (Some(high_day), Some(low_day)) = (weekly.high_weekday, weekly.low_weekday) {
        let width = match weekly.range_pct_of_adr {
            Some(p) => format!(" Amplitude de la semaine : {} % d'une journée moyenne.", p.round()),
            None => String::new(),
        };
        out.push(Observation {
            key: "weekly",
            text: format!(
                "Semaine en cours ({} séances, on est {}) : haut fait {}, bas fait {}.{width}",
                weekly.sessions, weekly.today_weekday, high_day, low_day
            ),
        });
    }

    // --- What still stands in the way ---------------------------------------

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

    fn levels_with(
        prev: (f64, f64),
        day: Option<(f64, f64, f64, f64)>,
        high_first: bool,
    ) -> DailyLevels {
        use crate::levels::PreviousDay;

        let ohlc = |o: f64, h: f64, l: f64, c: f64, high_at: &str, low_at: &str| Ohlc {
            open: o,
            high: h,
            low: l,
            close: c,
            range: h - l,
            bars: 200,
            high_at: utc(high_at),
            low_at: utc(low_at),
        };

        DailyLevels {
            symbol: "TEST".into(),
            date: date("2026-07-15"),
            timezone: "Europe/Paris",
            day: day.map(|(o, h, l, c)| {
                if high_first {
                    ohlc(o, h, l, c, "2026-07-15T09:00:00Z", "2026-07-15T15:00:00Z")
                } else {
                    ohlc(o, h, l, c, "2026-07-15T15:00:00Z", "2026-07-15T09:00:00Z")
                }
            }),
            previous_day: Some(PreviousDay {
                date: date("2026-07-14"),
                ohlc: ohlc(
                    prev.1,
                    prev.0,
                    prev.1,
                    prev.0,
                    "2026-07-14T09:00:00Z",
                    "2026-07-14T15:00:00Z",
                ),
            }),
            sessions: Vec::new(),
            opening_ranges: Vec::new(),
            sweeps: Vec::new(),
        }
    }

    // --- Daily bias ---------------------------------------------------------

    #[test]
    fn the_midpoint_is_the_middle_of_the_previous_range() {
        // Previous session 1000 -> 1100, so the 0.5 sits at 1050.
        let levels = levels_with((1100.0, 1000.0), Some((1060.0, 1080.0, 1055.0, 1075.0)), true);
        let bias = daily_bias(&levels);
        assert_eq!(bias.previous_mid, Some(1050.0));
        assert_eq!(bias.previous_high, Some(1100.0));
        assert_eq!(bias.previous_low, Some(1000.0));
    }

    #[test]
    fn a_session_whose_low_holds_above_the_midpoint_reads_as_continuation() {
        let levels = levels_with((1100.0, 1000.0), Some((1060.0, 1080.0, 1055.0, 1075.0)), true);
        let bias = daily_bias(&levels);

        assert_eq!(bias.open_above_mid, Some(true));
        assert_eq!(bias.held_above_mid, Some(true));
        assert_eq!(bias.read, BiasRead::ContinuationHaussiere);
        assert_eq!(bias.touched_pdh, Some(false));
    }

    #[test]
    fn taking_the_previous_high_then_closing_under_the_midpoint_is_a_reversal() {
        // High 1120 > PDH 1100, close 1020 < mid 1050.
        let levels = levels_with((1100.0, 1000.0), Some((1090.0, 1120.0, 1010.0, 1020.0)), true);
        let bias = daily_bias(&levels);

        assert_eq!(bias.touched_pdh, Some(true));
        assert_eq!(bias.reversal_from_high, Some(true));
        assert_eq!(bias.read, BiasRead::ReversalDepuisLeHaut);
    }

    /// The reversal signature must win even when the session opened on the
    /// "continuation" side — that is exactly the case the framework describes.
    #[test]
    fn the_reversal_signature_outranks_the_opening_side() {
        let levels = levels_with((1100.0, 1000.0), Some((1090.0, 1120.0, 1010.0, 1020.0)), true);
        let bias = daily_bias(&levels);
        assert_eq!(bias.open_above_mid, Some(true));
        assert_eq!(bias.read, BiasRead::ReversalDepuisLeHaut);
    }

    #[test]
    fn a_session_straddling_the_midpoint_gets_no_clean_read() {
        // Trades either side of 1050 and closes above, without taking PDH/PDL.
        let levels = levels_with((1100.0, 1000.0), Some((1040.0, 1070.0, 1020.0, 1060.0)), true);
        let bias = daily_bias(&levels);
        assert_eq!(bias.read, BiasRead::ZeroCinqTraverse);
    }

    /// Before the session has traded, the midpoint is still known — that is the
    /// pre-open case the screen has to serve.
    #[test]
    fn the_midpoint_is_available_before_the_session_trades() {
        let levels = levels_with((1100.0, 1000.0), None, true);
        let bias = daily_bias(&levels);

        assert_eq!(bias.previous_mid, Some(1050.0));
        assert!(bias.known_at_open);
        assert!(bias.open_above_mid.is_none());
        assert_eq!(bias.read, BiasRead::Indetermine);
    }

    #[test]
    fn which_extreme_came_first_follows_the_timestamps() {
        // Both sides taken; the high printed first.
        let up = levels_with((1100.0, 1000.0), Some((1050.0, 1120.0, 990.0, 1050.0)), true);
        assert_eq!(daily_bias(&up).first_taken, Some("PDH"));

        let down = levels_with((1100.0, 1000.0), Some((1050.0, 1120.0, 990.0, 1050.0)), false);
        assert_eq!(daily_bias(&down).first_taken, Some("PDL"));
    }

    // --- Weekly profile -----------------------------------------------------

    #[test]
    fn the_week_reports_which_day_made_the_high_and_the_low() {
        // Week of Monday 2026-07-13. Tuesday is the widest to the upside,
        // Thursday the lowest.
        let mut candles = Vec::new();
        candles.extend(session(date("2026-07-13"), 60, 1050.0, 1000.0));
        candles.extend(session(date("2026-07-14"), 60, 1200.0, 1010.0));
        candles.extend(session(date("2026-07-15"), 60, 1100.0, 1020.0));
        candles.extend(session(date("2026-07-16"), 60, 1090.0, 900.0));
        candles.sort_by_key(|c| c.ts);

        let weekly = weekly_profile(&candles, date("2026-07-16"), Some(100.0));

        assert_eq!(weekly.week_start, date("2026-07-13"));
        assert_eq!(weekly.sessions, 4);
        assert_eq!(weekly.today_weekday, "jeudi");
        assert_eq!(weekly.high, Some(1200.0));
        assert_eq!(weekly.high_weekday, Some("mardi"));
        assert_eq!(weekly.low, Some(900.0));
        assert_eq!(weekly.low_weekday, Some("jeudi"));
        assert_eq!(weekly.range, Some(300.0));
        assert_eq!(weekly.range_pct_of_adr, Some(300.0));
    }

    #[test]
    fn the_week_starts_on_monday_whatever_the_day_asked_for() {
        for (day, name) in [
            ("2026-07-13", "lundi"),
            ("2026-07-15", "mercredi"),
            ("2026-07-17", "vendredi"),
        ] {
            let weekly = weekly_profile(&[], date(day), None);
            assert_eq!(weekly.week_start, date("2026-07-13"));
            assert_eq!(weekly.today_weekday, name);
        }
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
