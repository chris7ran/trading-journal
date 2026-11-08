//! Prop firm rule tracking: profit target, drawdowns, consistency.
//!
//! Ported from the mobile app's `utils/propfirm.ts`, which computed all of
//! this on the phone from the full trade list. Moving it here has three
//! consequences worth stating, because they are the reason for the move:
//!
//! 1. The phone no longer has to download every trade to know whether a rule
//!    is close to breaking.
//! 2. The execution layer and the app now read the *same* numbers. A guard
//!    that refuses a trade because the daily loss is spent must agree with
//!    the screen that told the trader there was room left.
//! 3. It becomes testable without a simulator.
//!
//! Every threshold in `prop_rules` is a fraction of the **starting balance**,
//! not of current equity: `0.05` means five percent of the account as it was
//! on day one. That is how the firms write their own rules, and it matters —
//! a drawdown limit that shrank with the account would tighten exactly when
//! the trader could least afford it.

use serde::Serialize;

use crate::models::PropRule;

/// Fraction of a limit above which a warning is raised but nothing is broken.
const WARN_THRESHOLD: f64 = 0.8;

/// Default consistency rule when the account declares none: the best single
/// day may not exceed this share of total profit.
const DEFAULT_CONSISTENCY: f64 = 0.20;

/// One closed trade, reduced to what the rules care about.
#[derive(Debug, Clone)]
pub struct ClosedTrade {
    /// Calendar day the trade is attributed to, `YYYY-MM-DD`.
    pub day: String,
    /// Full timestamp, used only to order trades *inside* a day.
    pub at: String,
    pub pnl: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Ok,
    Warn,
    Danger,
}

#[derive(Debug, Clone, Serialize)]
pub struct PropAlert {
    pub level: AlertLevel,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PropStatus {
    pub start_balance: f64,
    pub net_pnl: f64,
    pub current_equity: f64,
    /// Highest equity ever reached, which trailing drawdowns are measured from.
    pub peak_equity: f64,

    /// Profit target as an amount, `None` when the account declares none.
    pub profit_target: Option<f64>,
    /// Progress toward the target, 0..1 and beyond.
    pub profit_progress: f64,
    pub target_reached: bool,

    /// Loss allowed from the peak, as an amount.
    pub max_drawdown: Option<f64>,
    /// How far below the peak we currently sit; never negative.
    pub current_drawdown: f64,
    pub drawdown_used: f64,

    /// Loss allowed today, as an amount.
    pub daily_limit: Option<f64>,
    pub today_pnl: f64,
    pub daily_used: f64,
    /// The day `today_pnl` was computed for, so a caller can tell whether the
    /// answer is about the day it thinks it is.
    pub today: String,

    /// Best single day may not exceed this share of total profit.
    pub consistency_limit: f64,
    pub best_day_pnl: f64,
    pub best_day: Option<String>,
    /// best_day_pnl / net_pnl, or 0 when the account is not in profit.
    pub consistency_pct: f64,
    pub consistency_ok: bool,
    /// Total profit needed before the best day stops breaching the rule.
    pub min_total_for_payout: f64,

    pub alerts: Vec<PropAlert>,
}

/// Evaluate every rule against a set of closed trades.
///
/// `today` is passed in rather than read from the clock so the result is
/// reproducible and testable. The caller decides which calendar day the
/// daily limit applies to — see the note on day boundaries below.
pub fn compute(
    trades: &[ClosedTrade],
    start_balance: f64,
    rule: Option<&PropRule>,
    today: &str,
) -> PropStatus {
    // Chronological order matters: the peak is a running maximum, so a
    // different order gives a different peak and therefore a different
    // drawdown. The mobile version sorted on the day alone, which left
    // trades within a day in the order the API returned them — newest first.
    // On a day that won then lost, that inverted the sequence and could
    // report a peak the account never reached. Sorting on the full timestamp
    // fixes it, at the cost of small differences against the old screen.
    let mut ordered: Vec<&ClosedTrade> = trades.iter().collect();
    ordered.sort_by(|a, b| a.day.cmp(&b.day).then_with(|| a.at.cmp(&b.at)));

    let mut equity = start_balance;
    let mut peak = start_balance;
    let mut net = 0.0;
    for trade in &ordered {
        equity += trade.pnl;
        net += trade.pnl;
        if equity > peak {
            peak = equity;
        }
    }

    let current_equity = start_balance + net;
    let current_drawdown = (peak - current_equity).max(0.0);

    let today_pnl: f64 = ordered
        .iter()
        .filter(|t| t.day == today)
        .map(|t| t.pnl)
        .sum();

    // Consistency is measured on the best *day*, not the best trade: the rule
    // exists to catch an account carried by one lucky session.
    let mut by_day: Vec<(String, f64)> = Vec::new();
    for trade in &ordered {
        match by_day.last_mut() {
            Some((day, total)) if *day == trade.day => *total += trade.pnl,
            _ => by_day.push((trade.day.clone(), trade.pnl)),
        }
    }
    let (best_day, best_day_pnl) = by_day
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, pnl)| *pnl > 0.0)
        .map(|(day, pnl)| (Some(day.clone()), *pnl))
        .unwrap_or((None, 0.0));

    let consistency_limit = rule
        .map(|r| r.consistency_rule_pct)
        .unwrap_or(DEFAULT_CONSISTENCY);
    let consistency_pct = if net > 0.0 { best_day_pnl / net } else { 0.0 };
    let consistency_ok = net <= 0.0 || best_day_pnl <= consistency_limit * net;
    let min_total_for_payout = if consistency_limit > 0.0 {
        best_day_pnl / consistency_limit
    } else {
        0.0
    };

    let profit_target = rule
        .and_then(|r| r.profit_target)
        .map(|f| start_balance * f);
    let max_drawdown = rule
        .and_then(|r| r.global_drawdown_max)
        .map(|f| start_balance * f);
    let daily_limit = rule
        .and_then(|r| r.daily_drawdown_max)
        .map(|f| start_balance * f);

    let profit_progress = ratio(net, profit_target);
    let drawdown_used = ratio(current_drawdown, max_drawdown);
    // Only a loss consumes the daily allowance; a winning day leaves it whole.
    let daily_loss = if today_pnl < 0.0 { -today_pnl } else { 0.0 };
    let daily_used = ratio(daily_loss, daily_limit);
    let target_reached = profit_target.is_some_and(|t| net >= t);

    let mut alerts = Vec::new();
    if max_drawdown.is_some() {
        if drawdown_used >= 1.0 {
            alerts.push(alert(AlertLevel::Danger, "Drawdown global DÉPASSÉ."));
        } else if drawdown_used >= WARN_THRESHOLD {
            alerts.push(alert(
                AlertLevel::Warn,
                "Drawdown global proche de la limite.",
            ));
        }
    }
    if daily_limit.is_some() {
        if daily_used >= 1.0 {
            alerts.push(alert(
                AlertLevel::Danger,
                "Limite de perte journalière DÉPASSÉE.",
            ));
        } else if daily_used >= WARN_THRESHOLD {
            alerts.push(alert(
                AlertLevel::Warn,
                "Perte journalière proche de la limite.",
            ));
        }
    }
    if target_reached {
        alerts.push(alert(AlertLevel::Ok, "Objectif de profit atteint."));
    }
    if net > 0.0 && !consistency_ok {
        alerts.push(alert(
            AlertLevel::Warn,
            &format!(
                "Consistance {}% > {}% — payout bloqué (total requis ≥ {}).",
                (consistency_pct * 100.0).round(),
                (consistency_limit * 100.0).round(),
                min_total_for_payout.round()
            ),
        ));
    }
    if alerts.is_empty() {
        alerts.push(alert(AlertLevel::Ok, "Tout est dans les règles."));
    }

    PropStatus {
        start_balance,
        net_pnl: net,
        current_equity,
        peak_equity: peak,
        profit_target,
        profit_progress,
        target_reached,
        max_drawdown,
        current_drawdown,
        drawdown_used,
        daily_limit,
        today_pnl,
        daily_used,
        today: today.to_string(),
        consistency_limit,
        best_day_pnl,
        best_day,
        consistency_pct,
        consistency_ok,
        min_total_for_payout,
        alerts,
    }
}

/// `value / limit`, or 0 when there is no limit or it is zero.
///
/// A missing limit must read as "nothing consumed" rather than as a division
/// by zero turning into an infinity that every comparison below would treat
/// as a breach.
fn ratio(value: f64, limit: Option<f64>) -> f64 {
    match limit {
        Some(l) if l > 0.0 => value / l,
        _ => 0.0,
    }
}

fn alert(level: AlertLevel, text: &str) -> PropAlert {
    PropAlert {
        level,
        text: text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> PropRule {
        PropRule {
            id: "r".into(),
            account_id: "a".into(),
            daily_drawdown_max: Some(0.05),
            global_drawdown_max: Some(0.10),
            profit_target: Some(0.10),
            min_trading_days: Some(4),
            consistency_rule_pct: 0.20,
            lot_size_max: None,
        }
    }

    fn trade(day: &str, at: &str, pnl: f64) -> ClosedTrade {
        ClosedTrade {
            day: day.into(),
            at: at.into(),
            pnl,
        }
    }

    #[test]
    fn an_untouched_account_breaks_nothing() {
        let status = compute(&[], 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.net_pnl, 0.0);
        assert_eq!(status.current_equity, 100_000.0);
        assert_eq!(status.peak_equity, 100_000.0);
        assert_eq!(status.current_drawdown, 0.0);
        assert_eq!(status.alerts.len(), 1);
        assert_eq!(status.alerts[0].level, AlertLevel::Ok);
    }

    #[test]
    fn drawdown_is_measured_from_the_peak_not_from_the_start() {
        // Up 8000, then down 5000: equity is still above the start, but the
        // account is 5000 below its high — which is what the rule watches.
        let trades = [
            trade("2026-11-02", "2026-11-02T10:00:00", 8_000.0),
            trade("2026-11-03", "2026-11-03T10:00:00", -5_000.0),
        ];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.peak_equity, 108_000.0);
        assert_eq!(status.current_equity, 103_000.0);
        assert_eq!(status.current_drawdown, 5_000.0);
        assert_eq!(status.drawdown_used, 0.5); // 5000 of the 10000 allowed
    }

    #[test]
    fn trades_are_ordered_within_a_day_so_the_peak_is_real() {
        // Won first, then lost: the peak really was reached.
        let won_then_lost = [
            trade("2026-11-02", "2026-11-02T09:00:00", 3_000.0),
            trade("2026-11-02", "2026-11-02T15:00:00", -1_000.0),
        ];
        // The same two trades handed over newest-first, as the API returns
        // them. The answer must not change.
        let reversed = [
            trade("2026-11-02", "2026-11-02T15:00:00", -1_000.0),
            trade("2026-11-02", "2026-11-02T09:00:00", 3_000.0),
        ];
        let a = compute(&won_then_lost, 100_000.0, Some(&rule()), "2026-11-09");
        let b = compute(&reversed, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(a.peak_equity, 103_000.0);
        assert_eq!(a.peak_equity, b.peak_equity);
        assert_eq!(a.current_drawdown, b.current_drawdown);
    }

    #[test]
    fn only_a_losing_day_consumes_the_daily_allowance() {
        let winning = [trade("2026-11-09", "2026-11-09T10:00:00", 2_000.0)];
        let status = compute(&winning, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.today_pnl, 2_000.0);
        assert_eq!(status.daily_used, 0.0);

        let losing = [trade("2026-11-09", "2026-11-09T10:00:00", -4_000.0)];
        let status = compute(&losing, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.daily_used, 0.8); // 4000 of the 5000 allowed
        assert!(status
            .alerts
            .iter()
            .any(|a| a.level == AlertLevel::Warn && a.text.contains("journalière")));
    }

    #[test]
    fn yesterdays_loss_does_not_count_against_today() {
        let trades = [trade("2026-11-08", "2026-11-08T10:00:00", -4_900.0)];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.today_pnl, 0.0);
        assert_eq!(status.daily_used, 0.0);
    }

    #[test]
    fn breaching_the_daily_limit_is_a_danger_not_a_warning() {
        let trades = [trade("2026-11-09", "2026-11-09T10:00:00", -5_100.0)];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert!(status.daily_used > 1.0);
        assert!(status
            .alerts
            .iter()
            .any(|a| a.level == AlertLevel::Danger));
    }

    #[test]
    fn one_outsized_day_blocks_the_payout() {
        // 9000 of the 10000 profit came from a single session: 90% against a
        // 20% rule.
        let trades = [
            trade("2026-11-02", "2026-11-02T10:00:00", 9_000.0),
            trade("2026-11-03", "2026-11-03T10:00:00", 500.0),
            trade("2026-11-04", "2026-11-04T10:00:00", 500.0),
        ];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.best_day_pnl, 9_000.0);
        assert_eq!(status.best_day.as_deref(), Some("2026-11-02"));
        assert!(!status.consistency_ok);
        assert_eq!(status.consistency_pct, 0.9);
        // 9000 may be at most 20% of the total, so 45000 would be needed.
        assert_eq!(status.min_total_for_payout, 45_000.0);
    }

    #[test]
    fn consistency_cannot_fail_on_a_losing_account() {
        // Dividing a best day by a negative total would produce a meaningless
        // ratio; the rule simply does not apply before there is profit.
        let trades = [
            trade("2026-11-02", "2026-11-02T10:00:00", 1_000.0),
            trade("2026-11-03", "2026-11-03T10:00:00", -4_000.0),
        ];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert!(status.net_pnl < 0.0);
        assert!(status.consistency_ok);
        assert_eq!(status.consistency_pct, 0.0);
    }

    #[test]
    fn an_account_with_no_rules_reports_figures_but_no_limits() {
        let trades = [trade("2026-11-02", "2026-11-02T10:00:00", -9_000.0)];
        let status = compute(&trades, 100_000.0, None, "2026-11-09");
        assert_eq!(status.current_drawdown, 9_000.0);
        assert!(status.max_drawdown.is_none());
        assert_eq!(status.drawdown_used, 0.0, "no limit means nothing consumed");
        assert_eq!(status.alerts.len(), 1);
        assert_eq!(status.alerts[0].level, AlertLevel::Ok);
    }

    #[test]
    fn reaching_the_target_is_reported() {
        let trades = [trade("2026-11-02", "2026-11-02T10:00:00", 10_500.0)];
        let status = compute(&trades, 100_000.0, Some(&rule()), "2026-11-09");
        assert!(status.target_reached);
        assert!(status.profit_progress > 1.0);
        assert!(status.alerts.iter().any(|a| a.text.contains("Objectif")));
    }

    #[test]
    fn thresholds_scale_with_the_starting_balance_not_with_equity() {
        // The same rules on a 10k account: five percent is 500, not 5000.
        let trades = [trade("2026-11-09", "2026-11-09T10:00:00", -400.0)];
        let status = compute(&trades, 10_000.0, Some(&rule()), "2026-11-09");
        assert_eq!(status.daily_limit, Some(500.0));
        assert_eq!(status.daily_used, 0.8);
    }
}
