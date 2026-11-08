//! `GET /trades/stats` — aggregated analytics over (optionally filtered) trades.
//!
//! Only trades with a non-NULL `pnl` count toward win/loss metrics; a trade is a
//! "win" when `pnl > 0` and a "loss" when `pnl < 0` (break-even trades count in
//! `total_trades` but neither wins nor losses).
//!
//! `group_by` cuts the same population along one dimension — day, week, month,
//! setup, direction, account, plan or symbol. That is what turns a single
//! number into a question worth asking: a 58% hit rate says little, the same
//! figure split by setup says which one is carrying the others.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::error::{AppError, AppResult};
use crate::models::{GroupStat, SymbolStat, TradeStats};
use crate::state::AppState;

/// Optional filters mirroring a subset of `GET /trades`.
#[derive(Debug, Deserialize)]
pub struct StatsFilters {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// One of the keys in `GROUPINGS`. Omitted means no breakdown.
    pub group_by: Option<String>,
}

/// The dimensions a caller may group along, and the SQL that produces each key.
///
/// A fixed table rather than anything built from the request: the grouping
/// expression goes into the query text itself, where a bound parameter cannot
/// reach, so the only safe input is a value we wrote ourselves.
///
/// The third field says whether the keys are chronological, which decides the
/// ordering — a month-by-month breakdown sorted by profit would be unreadable.
const GROUPINGS: &[(&str, &str, bool)] = &[
    ("symbol", "t.symbol", false),
    (
        "day",
        "substr(COALESCE(t.close_time, t.open_time, t.created_at), 1, 10)",
        true,
    ),
    (
        "week",
        "strftime('%Y-S%W', COALESCE(t.close_time, t.open_time, t.created_at))",
        true,
    ),
    (
        "month",
        "substr(COALESCE(t.close_time, t.open_time, t.created_at), 1, 7)",
        true,
    ),
    (
        "setup",
        "COALESCE(NULLIF(TRIM(t.setup_tag), ''), '(sans setup)')",
        false,
    ),
    (
        "direction",
        "COALESCE(NULLIF(TRIM(t.direction), ''), '(inconnue)')",
        false,
    ),
    ("account", "COALESCE(a.name, t.account_id)", false),
    ("plan", "COALESCE(t.plan_id, '(hors plan)')", false),
];

/// Raw aggregate row from the overall stats query.
#[derive(Debug, FromRow)]
struct StatsAgg {
    total: i64,
    wins: i64,
    losses: i64,
    total_pnl: f64,
    avg_win: f64,
    avg_loss: f64,
    gross_profit: f64,
    gross_loss: f64,
}

/// Raw aggregate row from the grouped query.
#[derive(Debug, FromRow)]
struct GroupAgg {
    key: String,
    trades: i64,
    wins: i64,
    losses: i64,
    pnl: f64,
    avg_win: f64,
    avg_loss: f64,
    gross_profit: f64,
    gross_loss: f64,
}

pub async fn trade_stats(
    State(state): State<AppState>,
    Query(filters): Query<StatsFilters>,
) -> AppResult<Json<TradeStats>> {
    let grouping = match filters.group_by.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(requested) => Some(resolve_grouping(requested)?),
    };

    // --- Overall aggregates ---
    // CAST(... AS REAL) forces a REAL storage class so sqlx always decodes f64.
    // Without it, a result set with only winning (or only losing) trades makes
    // the gross_loss/gross_profit SUM collapse to an INTEGER 0 -> decode error.
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT \
            COUNT(*) AS total, \
            COALESCE(SUM(CASE WHEN pnl > 0 THEN 1 ELSE 0 END), 0) AS wins, \
            COALESCE(SUM(CASE WHEN pnl < 0 THEN 1 ELSE 0 END), 0) AS losses, \
            CAST(COALESCE(SUM(pnl), 0) AS REAL) AS total_pnl, \
            CAST(COALESCE(AVG(CASE WHEN pnl > 0 THEN pnl END), 0) AS REAL) AS avg_win, \
            CAST(COALESCE(AVG(CASE WHEN pnl < 0 THEN pnl END), 0) AS REAL) AS avg_loss, \
            CAST(COALESCE(SUM(CASE WHEN pnl > 0 THEN pnl ELSE 0 END), 0) AS REAL) AS gross_profit, \
            CAST(COALESCE(SUM(CASE WHEN pnl < 0 THEN pnl ELSE 0 END), 0) AS REAL) AS gross_loss \
         FROM trades WHERE 1 = 1",
    );
    push_filters(&mut qb, &filters, "");
    let agg = qb
        .build_query_as::<StatsAgg>()
        .fetch_one(&state.pool)
        .await?;

    // --- Per-symbol breakdown ---
    let mut sqb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT symbol, \
            COUNT(*) AS trades, \
            COALESCE(SUM(CASE WHEN pnl > 0 THEN 1 ELSE 0 END), 0) AS wins, \
            CAST(COALESCE(SUM(pnl), 0) AS REAL) AS pnl \
         FROM trades WHERE 1 = 1",
    );
    push_filters(&mut sqb, &filters, "");
    sqb.push(" GROUP BY symbol ORDER BY pnl DESC");
    let by_symbol = sqb
        .build_query_as::<SymbolStat>()
        .fetch_all(&state.pool)
        .await?;

    // --- Requested breakdown ---
    let groups = match grouping {
        None => Vec::new(),
        Some((_, expression, chronological)) => {
            let mut gqb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT ");
            gqb.push(expression);
            gqb.push(
                " AS key, \
                 COUNT(*) AS trades, \
                 COALESCE(SUM(CASE WHEN t.pnl > 0 THEN 1 ELSE 0 END), 0) AS wins, \
                 COALESCE(SUM(CASE WHEN t.pnl < 0 THEN 1 ELSE 0 END), 0) AS losses, \
                 CAST(COALESCE(SUM(t.pnl), 0) AS REAL) AS pnl, \
                 CAST(COALESCE(AVG(CASE WHEN t.pnl > 0 THEN t.pnl END), 0) AS REAL) AS avg_win, \
                 CAST(COALESCE(AVG(CASE WHEN t.pnl < 0 THEN t.pnl END), 0) AS REAL) AS avg_loss, \
                 CAST(COALESCE(SUM(CASE WHEN t.pnl > 0 THEN t.pnl ELSE 0 END), 0) AS REAL) AS gross_profit, \
                 CAST(COALESCE(SUM(CASE WHEN t.pnl < 0 THEN t.pnl ELSE 0 END), 0) AS REAL) AS gross_loss \
                 FROM trades t LEFT JOIN accounts a ON a.id = t.account_id WHERE 1 = 1",
            );
            push_filters(&mut gqb, &filters, "t.");
            gqb.push(" GROUP BY ");
            gqb.push(expression);
            gqb.push(if chronological {
                " ORDER BY key ASC"
            } else {
                " ORDER BY pnl DESC"
            });

            gqb.build_query_as::<GroupAgg>()
                .fetch_all(&state.pool)
                .await?
                .into_iter()
                .map(|g| GroupStat {
                    win_rate: rate(g.wins, g.trades),
                    profit_factor: factor(g.gross_profit, g.gross_loss),
                    key: g.key,
                    trades: g.trades,
                    wins: g.wins,
                    losses: g.losses,
                    pnl: g.pnl,
                    avg_win: g.avg_win,
                    avg_loss: g.avg_loss,
                })
                .collect()
        }
    };

    Ok(Json(TradeStats {
        total_trades: agg.total,
        wins: agg.wins,
        losses: agg.losses,
        win_rate: rate(agg.wins, agg.total),
        total_pnl: agg.total_pnl,
        avg_win: agg.avg_win,
        avg_loss: agg.avg_loss,
        profit_factor: factor(agg.gross_profit, agg.gross_loss),
        gross_profit: agg.gross_profit,
        gross_loss: agg.gross_loss,
        by_symbol,
        groups,
        group_by: grouping.map(|(name, _, _)| name.to_string()),
    }))
}

/// Match a requested dimension against the whitelist, listing the valid ones
/// in the error — a caller that guessed wrong should not have to guess again.
fn resolve_grouping(requested: &str) -> AppResult<(&'static str, &'static str, bool)> {
    let wanted = requested.to_ascii_lowercase();
    GROUPINGS
        .iter()
        .find(|(name, _, _)| *name == wanted)
        .copied()
        .ok_or_else(|| {
            let known: Vec<&str> = GROUPINGS.iter().map(|(name, _, _)| *name).collect();
            AppError::BadRequest(format!(
                "group_by '{requested}' inconnu ; valeurs acceptées : {}",
                known.join(", ")
            ))
        })
}

/// Append the shared WHERE clauses. `prefix` is `"t."` for the query that
/// joins `accounts`, empty for the ones that do not — an unqualified
/// `symbol` would be ambiguous once a second table is in scope.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, filters: &StatsFilters, prefix: &str) {
    if let Some(account_id) = &filters.account_id {
        qb.push(format!(" AND {prefix}account_id = "))
            .push_bind(account_id.clone());
    }
    if let Some(symbol) = &filters.symbol {
        qb.push(format!(" AND {prefix}symbol = "))
            .push_bind(symbol.clone());
    }
    if let Some(from) = &filters.from {
        qb.push(format!(
            " AND COALESCE({prefix}open_time, {prefix}created_at) >= "
        ))
        .push_bind(from.clone());
    }
    if let Some(to) = &filters.to {
        qb.push(format!(
            " AND COALESCE({prefix}open_time, {prefix}created_at) <= "
        ))
        .push_bind(to.clone());
    }
}

fn rate(wins: i64, total: i64) -> f64 {
    if total > 0 {
        wins as f64 / total as f64
    } else {
        0.0
    }
}

fn factor(gross_profit: f64, gross_loss: f64) -> Option<f64> {
    if gross_loss != 0.0 {
        Some(gross_profit / gross_loss.abs())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grouping_resolves_to_itself() {
        for &(name, _, _) in GROUPINGS {
            let (resolved, _, _) = resolve_grouping(name).expect(name);
            assert_eq!(resolved, name);
        }
    }

    #[test]
    fn grouping_names_are_case_insensitive() {
        assert!(resolve_grouping("SETUP").is_ok());
        assert!(resolve_grouping("Month").is_ok());
    }

    #[test]
    fn an_unknown_grouping_names_the_valid_ones() {
        let error = resolve_grouping("semaine").unwrap_err().to_string();
        assert!(error.contains("semaine"));
        assert!(error.contains("week"), "the error must list what is allowed");
    }

    #[test]
    fn a_grouping_expression_never_comes_from_the_request() {
        // Guards the one place where text reaches the query instead of a
        // bound parameter: only the table's own strings may get there.
        assert!(resolve_grouping("symbol; DROP TABLE trades").is_err());
        assert!(resolve_grouping("1=1").is_err());
    }

    #[test]
    fn profit_factor_is_absent_rather_than_infinite_without_losses() {
        assert_eq!(factor(500.0, 0.0), None);
        assert_eq!(factor(500.0, -250.0), Some(2.0));
    }

    #[test]
    fn an_empty_group_has_a_zero_rate_not_a_division_by_zero() {
        assert_eq!(rate(0, 0), 0.0);
        assert_eq!(rate(3, 4), 0.75);
    }
}
