//! Daily levels: session ranges, opening ranges, previous-day extremes, sweeps.
//!
//! # Why sessions are defined in their *native* timezone
//!
//! It is tempting to write "London opens at 09:00 Paris time" and be done with
//! it. That works for most of the year and then quietly breaks: the EU and the
//! US switch on *different* dates, so the New York open sits an hour off its
//! usual Paris time for about three weeks in spring (US switches early March,
//! the EU end of March) and one week in autumn (EU switches late October, the
//! US the following weekend). A journal that silently mislabels a month of
//! opening ranges per year is worse than no journal.
//!
//! So every window below is expressed in the timezone the market actually lives
//! in — `Europe/London`, `America/New_York`, `Asia/Tokyo` — and converted to UTC
//! with full DST awareness at query time.
//!
//! # Two "Asian" sessions, on purpose
//!
//! Traders use the word for two different things:
//!   * the **Tokyo cash session** (09:00–15:00 JST), a real exchange session;
//!   * the **Asian range**, meaning everything from midnight London until the
//!     London open — the consolidation whose edges London so often sweeps.
//!
//! They overlap but are not the same window, and the second is the one that
//! matters for a London-open sweep. Both are computed.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;

/// The timezone the trading *day* is bucketed by. Chris trades from France, so
/// "Friday's levels" means the Paris calendar day.
pub const DAY_TZ: Tz = chrono_tz::Europe::Paris;

/// A single M5 candle, as read back from `candles_m5`.
#[derive(Debug, Clone, Copy)]
pub struct Candle {
    pub ts: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Aggregated OHLC over an arbitrary window.
#[derive(Debug, Clone, Serialize)]
pub struct Ohlc {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Range in price units — the number traders actually quote.
    pub range: f64,
    pub bars: usize,
    pub high_at: DateTime<Utc>,
    pub low_at: DateTime<Utc>,
}

/// A named time window plus whatever price action happened inside it.
#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub key: &'static str,
    pub label: &'static str,
    pub timezone: &'static str,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// `None` when the market was closed or no candle was received.
    #[serde(flatten)]
    pub ohlc: Option<Ohlc>,
}

/// "Did price take out this level during that window, and when?"
#[derive(Debug, Clone, Serialize)]
pub struct Sweep {
    pub key: &'static str,
    pub label: &'static str,
    pub level: f64,
    pub swept: bool,
    pub at: Option<DateTime<Utc>>,
    /// How far beyond the level price went, in price units. `0` when untouched.
    pub excursion: f64,
}

/// Everything computed for one symbol on one trading day.
#[derive(Debug, Clone, Serialize)]
pub struct DailyLevels {
    pub symbol: String,
    pub date: NaiveDate,
    pub timezone: &'static str,
    pub day: Option<Ohlc>,
    pub previous_day: Option<PreviousDay>,
    pub sessions: Vec<Window>,
    pub opening_ranges: Vec<Window>,
    pub sweeps: Vec<Sweep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviousDay {
    pub date: NaiveDate,
    #[serde(flatten)]
    pub ohlc: Ohlc,
}

/// Definition of a window, before it is resolved against a date.
struct WindowSpec {
    key: &'static str,
    label: &'static str,
    tz: Tz,
    tz_name: &'static str,
    from: (u32, u32),
    to: (u32, u32),
}

const SESSIONS: &[WindowSpec] = &[
    WindowSpec {
        key: "asia_range",
        label: "Range asiatique (pré-Londres)",
        tz: chrono_tz::Europe::London,
        tz_name: "Europe/London",
        from: (0, 0),
        to: (8, 0),
    },
    WindowSpec {
        key: "tokyo",
        label: "Tokyo (séance cash)",
        tz: chrono_tz::Asia::Tokyo,
        tz_name: "Asia/Tokyo",
        from: (9, 0),
        to: (15, 0),
    },
    WindowSpec {
        key: "london",
        label: "Londres",
        tz: chrono_tz::Europe::London,
        tz_name: "Europe/London",
        from: (8, 0),
        to: (16, 30),
    },
    WindowSpec {
        key: "newyork",
        label: "New York",
        tz: chrono_tz::America::New_York,
        tz_name: "America/New_York",
        from: (9, 30),
        to: (16, 0),
    },
];

const OPENING_RANGES: &[WindowSpec] = &[
    WindowSpec {
        key: "orb_london",
        label: "ORB Londres (15 min)",
        tz: chrono_tz::Europe::London,
        tz_name: "Europe/London",
        from: (8, 0),
        to: (8, 15),
    },
    WindowSpec {
        key: "orb_newyork",
        label: "ORB New York (15 min)",
        tz: chrono_tz::America::New_York,
        tz_name: "America/New_York",
        from: (9, 30),
        to: (9, 45),
    },
];

/// How many calendar days back we will look for the previous *trading* day.
/// Five covers a normal weekend plus a public holiday on either side, with the
/// Sunday-evening stub (see [`find_previous_day`]) now skipped rather than
/// consuming one of the attempts.
const PREVIOUS_DAY_LOOKBACK: i64 = 5;

/// Minimum M5 bars before a calendar day counts as a trading day: one hour.
const MIN_BARS_FOR_A_TRADING_DAY: usize = 12;

// --- Public API -------------------------------------------------------------

/// Compute every level for `symbol` on `date`.
///
/// `candles` must be **sorted ascending by timestamp** and must already cover
/// the previous few days (see [`utc_bounds_for_query`]). The ordering is not a
/// nicety: `open`, `close`, `high_at` and `low_at` are all derived from
/// traversal order, so an unsorted slice yields plausible-looking nonsense.
pub fn compute(symbol: &str, date: NaiveDate, candles: &[Candle]) -> DailyLevels {
    debug_assert!(
        candles.windows(2).all(|w| w[0].ts <= w[1].ts),
        "candles must be sorted ascending by timestamp"
    );

    let (day_start, day_end) = local_window(date, DAY_TZ, (0, 0), (24, 0));
    let day = aggregate(candles, day_start, day_end);

    let previous_day = find_previous_day(candles, date);

    let sessions: Vec<Window> = SESSIONS
        .iter()
        .map(|spec| resolve(spec, date, candles))
        .collect();
    let opening_ranges: Vec<Window> = OPENING_RANGES
        .iter()
        .map(|spec| resolve(spec, date, candles))
        .collect();

    let sweeps = compute_sweeps(candles, &sessions, previous_day.as_ref(), day_start, day_end);

    DailyLevels {
        symbol: symbol.to_string(),
        date,
        timezone: "Europe/Paris",
        day,
        previous_day,
        sessions,
        opening_ranges,
        sweeps,
    }
}

/// The Paris calendar day `date`, as a half-open UTC interval.
///
/// The trading day is a Paris day, not a UTC one, and the boundary moves with
/// DST — so anything that wants "exactly this session, nothing after it" has to
/// ask for it here rather than slicing UTC midnights by hand.
pub fn utc_day_bounds(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    local_window(date, DAY_TZ, (0, 0), (24, 0))
}

/// UTC range a caller must fetch from the database to be able to call
/// [`compute`] for `date`.
///
/// Wider than the Paris day on both sides: Tokyo's session for a given date
/// starts the previous UTC evening in some offsets, and the previous-trading-day
/// search needs several days of history.
pub fn utc_bounds_for_query(date: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let (start, end) = local_window(date, DAY_TZ, (0, 0), (24, 0));
    (
        start - Duration::days(PREVIOUS_DAY_LOOKBACK + 1),
        end + Duration::hours(6),
    )
}

/// UTC range covering `calendar_days` before `date`, for statistics that need
/// history (ADR, medians, trend).
pub fn utc_bounds_for_history(date: NaiveDate, calendar_days: i64) -> (DateTime<Utc>, DateTime<Utc>) {
    let (start, end) = local_window(date, DAY_TZ, (0, 0), (24, 0));
    (start - Duration::days(calendar_days), end + Duration::hours(6))
}

/// Daily OHLC for the `wanted` most recent trading days ending at `last`
/// (inclusive), oldest first.
///
/// Days without a real session are skipped using the same bar-count floor as
/// [`find_previous_day`], so weekends, holidays and the Sunday-evening reopen
/// never enter an average.
pub fn daily_series(candles: &[Candle], last: NaiveDate, wanted: usize) -> Vec<(NaiveDate, Ohlc)> {
    let mut out = Vec::with_capacity(wanted);

    // Scan back generously: 20 trading days span about 28 calendar days once
    // weekends are removed, plus room for a holiday run.
    let horizon = (wanted as i64) * 2 + 10;

    for back in 0..horizon {
        if out.len() == wanted {
            break;
        }
        let day = last - Duration::days(back);
        let (start, end) = local_window(day, DAY_TZ, (0, 0), (24, 0));
        if let Some(ohlc) = aggregate(candles, start, end) {
            if ohlc.bars >= MIN_BARS_FOR_A_TRADING_DAY {
                out.push((day, ohlc));
            }
        }
    }

    out.reverse(); // oldest first
    out
}

/// The UTC window of a named session or opening range on `date`.
///
/// Lets callers outside this module rebuild one window across many days —
/// for instance to compare today's Asian range with its own recent median.
pub fn window_bounds(key: &str, date: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    SESSIONS
        .iter()
        .chain(OPENING_RANGES.iter())
        .find(|spec| spec.key == key)
        .map(|spec| local_window(date, spec.tz, spec.from, spec.to))
}

/// Aggregate an arbitrary UTC window. See [`compute`] for the sort precondition.
pub fn aggregate_window(
    candles: &[Candle],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Option<Ohlc> {
    aggregate(candles, start, end)
}

// --- Internals --------------------------------------------------------------

fn resolve(spec: &WindowSpec, date: NaiveDate, candles: &[Candle]) -> Window {
    let (start, end) = local_window(date, spec.tz, spec.from, spec.to);
    Window {
        key: spec.key,
        label: spec.label,
        timezone: spec.tz_name,
        start,
        end,
        ohlc: aggregate(candles, start, end),
    }
}

/// Convert a local wall-clock window on `date` into UTC instants.
///
/// `to` may be `(24, 0)` to mean "midnight at the end of the day", which
/// `NaiveTime` cannot represent.
fn local_window(
    date: NaiveDate,
    tz: Tz,
    from: (u32, u32),
    to: (u32, u32),
) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = local_instant(date, tz, from);
    let end = if to.0 >= 24 {
        local_instant(date + Duration::days(1), tz, (to.0 - 24, to.1))
    } else {
        local_instant(date, tz, to)
    };
    (start, end)
}

/// Resolve one local wall-clock time to UTC, coping with DST.
///
/// Two edge cases exist and both are handled rather than unwrapped:
///   * **Gap** (spring forward): the wall-clock time never happened. We step
///     forward in 30-minute increments until we find a real instant.
///   * **Ambiguity** (autumn fall-back): the time happened twice. We take the
///     earlier one, which is the convention exchanges use.
fn local_instant(date: NaiveDate, tz: Tz, (hour, minute): (u32, u32)) -> DateTime<Utc> {
    let mut naive = date.and_time(
        NaiveTime::from_hms_opt(hour.min(23), minute, 0).unwrap_or_else(|| {
            NaiveTime::from_hms_opt(0, 0, 0).expect("midnight is a valid time")
        }),
    );

    for _ in 0..6 {
        match tz.from_local_datetime(&naive).earliest() {
            Some(dt) => return dt.with_timezone(&Utc),
            None => naive += Duration::minutes(30),
        }
    }

    // Unreachable in practice: no DST gap lasts three hours.
    DateTime::from_naive_utc_and_offset(naive, Utc)
}

/// Aggregate every candle whose OPEN time falls in `[start, end)`.
///
/// Half-open on purpose: the 08:00 candle belongs to the London session, the
/// 16:30 candle does not. Using the open time (rather than the close) matches
/// how MT5 stamps its bars, so a candle is never counted in two windows.
fn aggregate(candles: &[Candle], start: DateTime<Utc>, end: DateTime<Utc>) -> Option<Ohlc> {
    let mut iter = candles.iter().filter(|c| c.ts >= start && c.ts < end);
    let first = iter.next()?;

    let mut ohlc = Ohlc {
        open: first.open,
        high: first.high,
        low: first.low,
        close: first.close,
        range: first.high - first.low,
        bars: 1,
        high_at: first.ts,
        low_at: first.ts,
    };

    for c in iter {
        if c.high > ohlc.high {
            ohlc.high = c.high;
            ohlc.high_at = c.ts;
        }
        if c.low < ohlc.low {
            ohlc.low = c.low;
            ohlc.low_at = c.ts;
        }
        ohlc.close = c.close;
        ohlc.bars += 1;
    }
    ohlc.range = ohlc.high - ohlc.low;
    Some(ohlc)
}

/// Walk backwards day by day until we find a real trading session.
///
/// This skips weekends and holidays without hard-coding a calendar: a day with
/// no data simply is not a trading day.
///
/// The bar-count floor matters more than it looks. CFD and FX feeds reopen on
/// Sunday evening, which in Paris terms lands a handful of bars in the Sunday
/// 23:00–24:00 slice. Accepting "at least one candle" would make Monday's PDH
/// and PDL the extremes of that one-hour stub instead of Friday's actual range —
/// silently wrong on precisely the day those levels get used most.
fn find_previous_day(candles: &[Candle], date: NaiveDate) -> Option<PreviousDay> {
    for back in 1..=PREVIOUS_DAY_LOOKBACK {
        let candidate = date - Duration::days(back);
        let (start, end) = local_window(candidate, DAY_TZ, (0, 0), (24, 0));
        if let Some(ohlc) = aggregate(candles, start, end) {
            if ohlc.bars >= MIN_BARS_FOR_A_TRADING_DAY {
                return Some(PreviousDay {
                    date: candidate,
                    ohlc,
                });
            }
        }
    }
    None
}

fn compute_sweeps(
    candles: &[Candle],
    sessions: &[Window],
    previous_day: Option<&PreviousDay>,
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
) -> Vec<Sweep> {
    let find = |key: &str| sessions.iter().find(|w| w.key == key);
    let mut sweeps = Vec::new();

    // London taking out the Asian range — the classic liquidity grab at the open.
    if let (Some(asia), Some(london)) = (find("asia_range"), find("london")) {
        if let Some(range) = &asia.ohlc {
            sweeps.push(sweep_above(
                candles,
                london.start,
                london.end,
                range.high,
                "london_sweep_asia_high",
                "Londres prend le haut du range asiatique",
            ));
            sweeps.push(sweep_below(
                candles,
                london.start,
                london.end,
                range.low,
                "london_sweep_asia_low",
                "Londres prend le bas du range asiatique",
            ));
        }
    }

    // New York taking out London's extremes.
    //
    // Subtle but decisive: the London and New York windows *overlap* (NY 09:30
    // is 14:30 London, London runs to 16:30). Using the full London range as the
    // level would make the sweep undetectable during that overlap, because a
    // candle that breaks the high is itself part of the high. That would silence
    // exactly the case worth watching — New York taking London's range at the
    // open. So the level is the London **morning** range, up to the NY open.
    if let (Some(london), Some(ny)) = (find("london"), find("newyork")) {
        if let Some(morning) = aggregate(candles, london.start, ny.start) {
            sweeps.push(sweep_above(
                candles,
                ny.start,
                ny.end,
                morning.high,
                "ny_sweep_london_high",
                "New York prend le haut de la matinée de Londres",
            ));
            sweeps.push(sweep_below(
                candles,
                ny.start,
                ny.end,
                morning.low,
                "ny_sweep_london_low",
                "New York prend le bas de la matinée de Londres",
            ));
        }
    }

    // The day against the previous day's extremes (PDH / PDL).
    if let Some(prev) = previous_day {
        sweeps.push(sweep_above(
            candles,
            day_start,
            day_end,
            prev.ohlc.high,
            "day_sweep_pdh",
            "Le jour prend le haut de la veille (PDH)",
        ));
        sweeps.push(sweep_below(
            candles,
            day_start,
            day_end,
            prev.ohlc.low,
            "day_sweep_pdl",
            "Le jour prend le bas de la veille (PDL)",
        ));
    }

    sweeps
}

fn sweep_above(
    candles: &[Candle],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    level: f64,
    key: &'static str,
    label: &'static str,
) -> Sweep {
    let mut at = None;
    let mut peak = f64::NEG_INFINITY;
    for c in candles.iter().filter(|c| c.ts >= start && c.ts < end) {
        if c.high > level {
            at.get_or_insert(c.ts);
            peak = peak.max(c.high);
        }
    }
    Sweep {
        key,
        label,
        level,
        swept: at.is_some(),
        at,
        excursion: if at.is_some() { peak - level } else { 0.0 },
    }
}

fn sweep_below(
    candles: &[Candle],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    level: f64,
    key: &'static str,
    label: &'static str,
) -> Sweep {
    let mut at = None;
    let mut trough = f64::INFINITY;
    for c in candles.iter().filter(|c| c.ts >= start && c.ts < end) {
        if c.low < level {
            at.get_or_insert(c.ts);
            trough = trough.min(c.low);
        }
    }
    Sweep {
        key,
        label,
        level,
        swept: at.is_some(),
        at,
        excursion: if at.is_some() { level - trough } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn c(ts: &str, o: f64, h: f64, l: f64, cl: f64) -> Candle {
        Candle {
            ts: utc(ts),
            open: o,
            high: h,
            low: l,
            close: cl,
        }
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    // --- DST: the reason this module exists ---------------------------------

    #[test]
    fn london_open_is_0700_utc_in_summer_and_0800_utc_in_winter() {
        let (summer, _) = local_window(
            date("2026-07-15"),
            chrono_tz::Europe::London,
            (8, 0),
            (16, 30),
        );
        let (winter, _) = local_window(
            date("2026-12-15"),
            chrono_tz::Europe::London,
            (8, 0),
            (16, 30),
        );
        assert_eq!(summer, utc("2026-07-15T07:00:00Z"));
        assert_eq!(winter, utc("2026-12-15T08:00:00Z"));
    }

    #[test]
    fn ny_open_is_1330_utc_in_summer_and_1430_utc_in_winter() {
        let (summer, _) = local_window(
            date("2026-07-15"),
            chrono_tz::America::New_York,
            (9, 30),
            (16, 0),
        );
        let (winter, _) = local_window(
            date("2026-12-15"),
            chrono_tz::America::New_York,
            (9, 30),
            (16, 0),
        );
        assert_eq!(summer, utc("2026-07-15T13:30:00Z"));
        assert_eq!(winter, utc("2026-12-15T14:30:00Z"));
    }

    /// 24 March 2026 sits in the three-week hole where the US has already moved
    /// to EDT (second Sunday of March, the 8th) but Europe has not (last Sunday
    /// of March, the 29th).
    ///
    /// This is the case that breaks hand-rolled offsets. For most of the year
    /// the London open and the New York open are 6h30 apart in UTC terms; in
    /// this window they are 5h30 apart. Anyone who hard-coded "NY opens at
    /// 15:30 Paris" gets three weeks of opening ranges silently wrong.
    #[test]
    fn handles_the_weeks_when_uk_and_us_dst_disagree() {
        let d = date("2026-03-24");
        let (london, _) = local_window(d, chrono_tz::Europe::London, (8, 0), (16, 30));
        let (ny, _) = local_window(d, chrono_tz::America::New_York, (9, 30), (16, 0));

        assert_eq!(london, utc("2026-03-24T08:00:00Z")); // still GMT
        assert_eq!(ny, utc("2026-03-24T13:30:00Z")); // already EDT

        let gap = (ny - london).num_minutes();
        assert_eq!(gap, 330, "5h30 apart, not the usual 6h30");
    }

    /// The autumn mirror: 28 October 2026 sits between the EU switch (last
    /// Sunday of October, the 25th) and the US one (first Sunday of November,
    /// the 1st), so Europe is back on standard time while the US is still on
    /// DST. Same 5h30 gap as late March, from the opposite direction.
    #[test]
    fn handles_the_autumn_dst_gap_too() {
        let d = date("2026-10-28");
        let (london, _) = local_window(d, chrono_tz::Europe::London, (8, 0), (16, 30));
        let (ny, _) = local_window(d, chrono_tz::America::New_York, (9, 30), (16, 0));

        assert_eq!(london, utc("2026-10-28T08:00:00Z")); // back on GMT
        assert_eq!(ny, utc("2026-10-28T13:30:00Z")); // still EDT
        assert_eq!((ny - london).num_minutes(), 330);
    }

    #[test]
    fn tokyo_never_shifts_because_japan_has_no_dst() {
        for day in ["2026-01-15", "2026-07-15"] {
            let (start, _) = local_window(date(day), chrono_tz::Asia::Tokyo, (9, 0), (15, 0));
            assert_eq!(start.format("%H:%M").to_string(), "00:00");
        }
    }

    #[test]
    fn day_window_ends_at_local_midnight_not_utc_midnight() {
        let (start, end) = local_window(date("2026-07-15"), DAY_TZ, (0, 0), (24, 0));
        assert_eq!(start, utc("2026-07-14T22:00:00Z"));
        assert_eq!(end, utc("2026-07-15T22:00:00Z"));
    }

    // --- Aggregation --------------------------------------------------------

    #[test]
    fn aggregate_uses_first_open_last_close_and_tracks_extremes() {
        let candles = vec![
            c("2026-07-15T07:00:00Z", 100.0, 102.0, 99.0, 101.0),
            c("2026-07-15T07:05:00Z", 101.0, 106.0, 100.0, 105.0),
            c("2026-07-15T07:10:00Z", 105.0, 105.5, 97.0, 98.0),
        ];
        let o = aggregate(
            &candles,
            utc("2026-07-15T07:00:00Z"),
            utc("2026-07-15T08:00:00Z"),
        )
        .unwrap();

        assert_eq!(o.open, 100.0);
        assert_eq!(o.close, 98.0);
        assert_eq!(o.high, 106.0);
        assert_eq!(o.low, 97.0);
        assert_eq!(o.range, 9.0);
        assert_eq!(o.bars, 3);
        assert_eq!(o.high_at, utc("2026-07-15T07:05:00Z"));
        assert_eq!(o.low_at, utc("2026-07-15T07:10:00Z"));
    }

    #[test]
    fn window_is_half_open_so_a_candle_never_lands_in_two_sessions() {
        let candles = vec![
            c("2026-07-15T07:00:00Z", 100.0, 100.0, 100.0, 100.0),
            c("2026-07-15T08:00:00Z", 200.0, 200.0, 200.0, 200.0),
        ];
        let o = aggregate(
            &candles,
            utc("2026-07-15T07:00:00Z"),
            utc("2026-07-15T08:00:00Z"),
        )
        .unwrap();
        assert_eq!(o.bars, 1);
        assert_eq!(o.high, 100.0);
    }

    #[test]
    fn aggregate_returns_none_when_the_market_was_closed() {
        let candles = vec![c("2026-07-15T07:00:00Z", 100.0, 100.0, 100.0, 100.0)];
        assert!(aggregate(
            &candles,
            utc("2026-07-15T20:00:00Z"),
            utc("2026-07-15T21:00:00Z")
        )
        .is_none());
    }

    // --- Sweeps -------------------------------------------------------------

    #[test]
    fn detects_a_sweep_and_reports_when_and_how_far() {
        let candles = vec![
            c("2026-07-15T07:00:00Z", 100.0, 101.0, 99.0, 100.0),
            c("2026-07-15T07:05:00Z", 100.0, 104.0, 100.0, 103.0),
            c("2026-07-15T07:10:00Z", 103.0, 106.0, 102.0, 104.0),
        ];
        let s = sweep_above(
            &candles,
            utc("2026-07-15T07:00:00Z"),
            utc("2026-07-15T08:00:00Z"),
            102.0,
            "k",
            "l",
        );
        assert!(s.swept);
        assert_eq!(s.at, Some(utc("2026-07-15T07:05:00Z"))); // first breach, not the biggest
        assert_eq!(s.excursion, 4.0); // furthest was 106
    }

    #[test]
    fn touching_a_level_exactly_is_not_a_sweep() {
        let candles = vec![c("2026-07-15T07:00:00Z", 100.0, 102.0, 99.0, 100.0)];
        let s = sweep_above(
            &candles,
            utc("2026-07-15T07:00:00Z"),
            utc("2026-07-15T08:00:00Z"),
            102.0,
            "k",
            "l",
        );
        assert!(!s.swept);
        assert_eq!(s.excursion, 0.0);
    }

    // --- Previous day -------------------------------------------------------

    /// Build `n` consecutive M5 candles from `start`, all with the same prices
    /// except an optional spike on the first bar.
    fn session(start: &str, n: usize, high: f64, low: f64) -> Vec<Candle> {
        let t0 = utc(start);
        (0..n)
            .map(|i| Candle {
                ts: t0 + Duration::minutes(5 * i as i64),
                open: 100.0,
                high: if i == 0 { high } else { 100.5 },
                low: if i == 0 { low } else { 99.5 },
                close: 100.0,
            })
            .collect()
    }

    #[test]
    fn previous_day_skips_the_weekend() {
        // Monday 2026-07-20; the only real session is Friday 2026-07-17.
        let candles = session("2026-07-17T12:00:00Z", 12, 110.0, 90.0);
        let prev = find_previous_day(&candles, date("2026-07-20")).unwrap();
        assert_eq!(prev.date, date("2026-07-17"));
        assert_eq!(prev.ohlc.high, 110.0);
        assert_eq!(prev.ohlc.low, 90.0);
    }

    #[test]
    fn previous_day_ignores_the_sunday_evening_reopen_stub() {
        // Monday 2026-07-20. The CFD feed reopened Sunday 2026-07-19 at 23:00
        // Paris with a few bars; Friday is the real previous session.
        let mut candles = session("2026-07-17T12:00:00Z", 12, 110.0, 90.0);
        candles.extend(session("2026-07-19T21:05:00Z", 4, 500.0, 1.0)); // 23:05 Paris
        candles.sort_by_key(|c| c.ts);

        let prev = find_previous_day(&candles, date("2026-07-20")).unwrap();
        assert_eq!(prev.date, date("2026-07-17"), "must not latch onto Sunday");
        assert_eq!(prev.ohlc.high, 110.0);
        assert_eq!(prev.ohlc.low, 90.0);
    }

    #[test]
    fn previous_day_is_none_when_history_is_missing() {
        assert!(find_previous_day(&[], date("2026-07-20")).is_none());
    }

    // --- End to end ---------------------------------------------------------

    #[test]
    fn computes_the_london_opening_range_from_the_first_three_candles() {
        // 15 July 2026, BST: London opens 07:00Z. ORB = 07:00, 07:05, 07:10.
        let candles = vec![
            c("2026-07-15T07:00:00Z", 100.0, 103.0, 99.0, 102.0),
            c("2026-07-15T07:05:00Z", 102.0, 105.0, 101.0, 104.0),
            c("2026-07-15T07:10:00Z", 104.0, 104.5, 100.0, 101.0),
            c("2026-07-15T07:15:00Z", 101.0, 120.0, 80.0, 110.0), // outside the ORB
        ];
        let levels = compute("GER40", date("2026-07-15"), &candles);
        let orb = levels
            .opening_ranges
            .iter()
            .find(|w| w.key == "orb_london")
            .unwrap();
        let ohlc = orb.ohlc.as_ref().unwrap();

        assert_eq!(ohlc.bars, 3);
        assert_eq!(ohlc.high, 105.0);
        assert_eq!(ohlc.low, 99.0);
        assert_eq!(ohlc.range, 6.0);
    }

    #[test]
    fn london_sweeping_the_asian_high_is_reported() {
        let candles = vec![
            // Asian range, 00:00–08:00 London (23:00Z prev day – 07:00Z in BST).
            c("2026-07-15T02:00:00Z", 100.0, 101.0, 99.0, 100.0),
            c("2026-07-15T03:00:00Z", 100.0, 100.5, 99.5, 100.0),
            // London open sweeps above 101.
            c("2026-07-15T07:00:00Z", 100.0, 103.0, 100.0, 102.0),
        ];
        let levels = compute("GER40", date("2026-07-15"), &candles);
        let sweep = levels
            .sweeps
            .iter()
            .find(|s| s.key == "london_sweep_asia_high")
            .unwrap();

        assert_eq!(sweep.level, 101.0);
        assert!(sweep.swept);
        assert_eq!(sweep.at, Some(utc("2026-07-15T07:00:00Z")));
    }

    /// Regression guard for the overlap trap: London runs to 16:30 local, well
    /// past the New York open, so using the *full* London range as the level
    /// would make the sweep mathematically impossible to detect during the
    /// overlap — the breaking candle would be part of its own level.
    #[test]
    fn new_york_sweeping_the_london_morning_high_is_detected() {
        // 15 July 2026 (BST/EDT): London 07:00–15:30Z, NY opens 13:30Z.
        let candles = vec![
            c("2026-07-15T07:00:00Z", 100.0, 102.0, 99.0, 101.0),
            c("2026-07-15T08:00:00Z", 101.0, 105.0, 100.0, 103.0), // morning high
            c("2026-07-15T12:00:00Z", 103.0, 104.0, 102.0, 103.0),
            c("2026-07-15T14:00:00Z", 103.0, 108.0, 103.0, 107.0), // NY takes it
        ];
        let levels = compute("GER40", date("2026-07-15"), &candles);
        let sweep = levels
            .sweeps
            .iter()
            .find(|s| s.key == "ny_sweep_london_high")
            .unwrap();

        assert_eq!(sweep.level, 105.0, "level is the pre-NY London high");
        assert!(sweep.swept);
        assert_eq!(sweep.at, Some(utc("2026-07-15T14:00:00Z")));
        assert_eq!(sweep.excursion, 3.0);
    }

    #[test]
    fn a_day_with_no_data_produces_empty_but_valid_output() {
        let levels = compute("GER40", date("2026-07-15"), &[]);
        assert!(levels.day.is_none());
        assert!(levels.previous_day.is_none());
        assert_eq!(levels.sessions.len(), 4);
        assert!(levels.sessions.iter().all(|s| s.ohlc.is_none()));
        assert!(levels.sweeps.is_empty());
    }

    #[test]
    fn query_bounds_cover_the_day_plus_enough_history_for_the_previous_day() {
        let (from, to) = utc_bounds_for_query(date("2026-07-15"));
        assert!(from <= utc("2026-07-10T00:00:00Z"));
        assert!(to >= utc("2026-07-15T22:00:00Z"));
    }
}
