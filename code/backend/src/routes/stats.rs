//! `GET /trades/stats` — aggregated analytics over (optionally filtered) trades.
//!
//! Only trades with a non-NULL `pnl` count toward win/loss metrics; a trade is a
//! "win" when `pnl > 0` and a "loss" when `pnl < 0` (break-even trades count in
//! `total_trades` but neither wins nor losses).

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::{FromRow, QueryBuilder, Sqlite};

use crate::error::AppResult;
use crate::models::{SymbolStat, TradeStats};
use crate::state::AppState;

/// Optional filters mirroring a subset of `GET /trades`.
#[derive(Debug, Deserialize)]
pub struct StatsFilters {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

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

pub async fn trade_stats(
    State(state): State<AppState>,
    Query(filters): Query<StatsFilters>,
) -> AppResult<Json<TradeStats>> {
    // Build the shared WHERE clause once and reuse it for both queries.
    let push_filters = |qb: &mut QueryBuilder<Sqlite>| {
        if let Some(account_id) = &filters.account_id {
            qb.push(" AND account_id = ").push_bind(account_id.clone());
        }
        if let Some(symbol) = &filters.symbol {
            qb.push(" AND symbol = ").push_bind(symbol.clone());
        }
        if let Some(from) = &filters.from {
            qb.push(" AND COALESCE(open_time, created_at) >= ")
                .push_bind(from.clone());
        }
        if let Some(to) = &filters.to {
            qb.push(" AND COALESCE(open_time, created_at) <= ")
                .push_bind(to.clone());
        }
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
    push_filters(&mut qb);
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
    push_filters(&mut sqb);
    sqb.push(" GROUP BY symbol ORDER BY pnl DESC");
    let by_symbol = sqb
        .build_query_as::<SymbolStat>()
        .fetch_all(&state.pool)
        .await?;

    let win_rate = if agg.total > 0 {
        agg.wins as f64 / agg.total as f64
    } else {
        0.0
    };

    let profit_factor = if agg.gross_loss != 0.0 {
        Some(agg.gross_profit / agg.gross_loss.abs())
    } else {
        None
    };

    Ok(Json(TradeStats {
        total_trades: agg.total,
        wins: agg.wins,
        losses: agg.losses,
        win_rate,
        total_pnl: agg.total_pnl,
        avg_win: agg.avg_win,
        avg_loss: agg.avg_loss,
        profit_factor,
        gross_profit: agg.gross_profit,
        gross_loss: agg.gross_loss,
        by_symbol,
    }))
}
