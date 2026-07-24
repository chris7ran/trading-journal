//! Domain models and request/response DTOs.
//!
//! Timestamps are stored and exchanged as ISO-8601 strings (UTC, e.g.
//! `2026-06-20T13:45:00`). Keeping them as `String` avoids brittle
//! database<->chrono decoding for the SQLite TEXT columns; richer typing can be
//! layered on later if analytics needs it.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// A trade as stored in the database and returned by the API.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Trade {
    pub id: String,
    pub account_id: String,
    pub symbol: String,
    pub direction: Option<String>,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
    pub open_price: Option<f64>,
    pub close_price: Option<f64>,
    pub lot_size: Option<f64>,
    pub pnl: Option<f64>,
    pub pnl_pct: Option<f64>,
    pub commission: Option<f64>,
    pub swap: Option<f64>,
    pub setup_tag: Option<String>,
    pub emotion_tag: Option<String>,
    pub notes: Option<String>,
    pub screenshot_url: Option<String>,
    pub mt5_ticket: Option<String>,
    // Post-trade review (1/0/NULL). setup_tag holds the chosen setup name.
    pub followed_plan: Option<bool>,
    pub respected_sl: Option<bool>,
    pub pattern_valid: Option<bool>,
    pub thesis_worked: Option<bool>,
    pub good_exit: Option<bool>,
    pub created_at: String,
}

/// Payload for `POST /trades` (manual entry).
#[derive(Debug, Deserialize)]
pub struct NewTrade {
    /// Defaults to the seeded "default" account when omitted.
    pub account_id: Option<String>,
    pub symbol: String,
    pub direction: Option<String>,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
    pub open_price: Option<f64>,
    pub close_price: Option<f64>,
    pub lot_size: Option<f64>,
    pub pnl: Option<f64>,
    pub pnl_pct: Option<f64>,
    pub commission: Option<f64>,
    pub swap: Option<f64>,
    pub setup_tag: Option<String>,
    pub emotion_tag: Option<String>,
    pub notes: Option<String>,
    pub screenshot_url: Option<String>,
    pub mt5_ticket: Option<String>,
    pub followed_plan: Option<bool>,
    pub respected_sl: Option<bool>,
    pub pattern_valid: Option<bool>,
    pub thesis_worked: Option<bool>,
    pub good_exit: Option<bool>,
}

/// Payload for `PUT /trades/:id`. Every field is optional; only provided fields
/// are updated (partial update / PATCH-like semantics).
#[derive(Debug, Deserialize, Default)]
pub struct UpdateTrade {
    pub symbol: Option<String>,
    pub direction: Option<String>,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
    pub open_price: Option<f64>,
    pub close_price: Option<f64>,
    pub lot_size: Option<f64>,
    pub pnl: Option<f64>,
    pub pnl_pct: Option<f64>,
    pub commission: Option<f64>,
    pub swap: Option<f64>,
    pub setup_tag: Option<String>,
    pub emotion_tag: Option<String>,
    pub notes: Option<String>,
    pub screenshot_url: Option<String>,
    pub followed_plan: Option<bool>,
    pub respected_sl: Option<bool>,
    pub pattern_valid: Option<bool>,
    pub thesis_worked: Option<bool>,
    pub good_exit: Option<bool>,
}

/// Query filters for `GET /trades`.
#[derive(Debug, Deserialize)]
pub struct TradeFilters {
    pub account_id: Option<String>,
    pub symbol: Option<String>,
    pub direction: Option<String>,
    /// Inclusive lower bound on `open_time` (ISO-8601).
    pub from: Option<String>,
    /// Inclusive upper bound on `open_time` (ISO-8601).
    pub to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- Accounts ----------------------------------------------------------------

/// A trading account as stored and returned.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub broker: String,
    pub balance: Option<f64>,
    pub currency: String,
    pub is_funded: bool,
    pub created_at: String,
}

/// Payload for `POST /accounts`.
#[derive(Debug, Deserialize)]
pub struct NewAccount {
    pub name: String,
    pub broker: Option<String>,
    pub balance: Option<f64>,
    pub currency: Option<String>,
    pub is_funded: Option<bool>,
}

/// Payload for `PUT /accounts/:id` (partial update).
#[derive(Debug, Deserialize, Default)]
pub struct UpdateAccount {
    pub name: Option<String>,
    pub balance: Option<f64>,
    pub currency: Option<String>,
    pub is_funded: Option<bool>,
}

// --- Prop firm rules ---------------------------------------------------------

/// Prop firm rules attached to an account.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PropRule {
    pub id: String,
    pub account_id: String,
    pub daily_drawdown_max: Option<f64>,
    pub global_drawdown_max: Option<f64>,
    pub profit_target: Option<f64>,
    pub min_trading_days: Option<i64>,
    pub consistency_rule_pct: f64,
    pub lot_size_max: Option<f64>,
}

/// Payload for `PUT /accounts/:id/rules` (insert-or-update).
#[derive(Debug, Deserialize)]
pub struct UpsertPropRule {
    pub daily_drawdown_max: Option<f64>,
    pub global_drawdown_max: Option<f64>,
    pub profit_target: Option<f64>,
    pub min_trading_days: Option<i64>,
    /// Defaults to 0.20 (20%) when omitted, matching the schema default.
    pub consistency_rule_pct: Option<f64>,
    pub lot_size_max: Option<f64>,
}

// --- Analytics ---------------------------------------------------------------

/// Aggregated trade statistics returned by `GET /trades/stats`.
#[derive(Debug, Serialize)]
pub struct TradeStats {
    pub total_trades: i64,
    pub wins: i64,
    pub losses: i64,
    /// Fraction in [0, 1]; 0 when there are no closed trades.
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    /// gross_profit / |gross_loss|; `None` when there are no losses.
    pub profit_factor: Option<f64>,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub by_symbol: Vec<SymbolStat>,
}

/// Per-symbol breakdown row.
#[derive(Debug, Serialize, FromRow)]
pub struct SymbolStat {
    pub symbol: String,
    pub trades: i64,
    pub wins: i64,
    pub pnl: f64,
}

// --- Setups ------------------------------------------------------------------

/// A named trading setup / pattern with its own rules and structured fields.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Setup {
    pub id: String,
    pub name: String,
    pub rules: Option<String>,
    pub description: Option<String>,
    pub target_entry: Option<String>,
    pub target_exit: Option<String>,
    pub stop_loss: Option<String>,
    pub created_at: String,
}

/// Payload for `POST /setups`.
#[derive(Debug, Deserialize)]
pub struct NewSetup {
    pub name: String,
    #[serde(default)]
    pub rules: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub target_entry: Option<String>,
    #[serde(default)]
    pub target_exit: Option<String>,
    #[serde(default)]
    pub stop_loss: Option<String>,
}

/// Payload for `PUT /setups/:id` — any field omitted keeps its current value.
#[derive(Debug, Deserialize)]
pub struct UpdateSetup {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rules: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub target_entry: Option<String>,
    #[serde(default)]
    pub target_exit: Option<String>,
    #[serde(default)]
    pub stop_loss: Option<String>,
}

// --- Macro terminal ----------------------------------------------------------

/// Economic calendar event (high/medium impact only).
#[derive(Debug, Clone, Serialize)]
pub struct EcoEvent {
    pub title: String,
    pub currency: String, // e.g. USD, EUR
    pub impact: String,   // "red" (high) or "orange" (medium)
    pub date: String,     // ISO-8601 with offset
    pub forecast: Option<String>,
    pub previous: Option<String>,
}

/// Aggregated news headline.
#[derive(Debug, Clone, Serialize)]
pub struct NewsItem {
    pub title: String,
    pub url: String,
    pub source: String,
    pub published_at: Option<String>,
    pub sentiment: String, // "bullish" | "bearish" | "neutral"
}

/// Weekly CFTC Commitments of Traders positioning for one futures contract
/// (leveraged funds = speculative hedge funds).
#[derive(Debug, Clone, Serialize)]
pub struct CotEntry {
    pub marche: String, // "S&P 500" | "EUR" ...
    pub net: i64,       // long - short (leveraged funds)
    pub chg_hebdo: i64, // week-over-week change in net
    pub date: String,   // report date, YYYY-MM-DD
}

/// Macro economic indicator (World Bank) or market instrument (Stooq), with a
/// small history for a sparkline.
#[derive(Debug, Clone, Serialize)]
pub struct EconIndicator {
    pub label: String,  // "Inflation (CPI)" | "Or (XAU/USD)"
    pub region: String, // "États-Unis" | "Or" | "Taux US" ...
    pub unit: String,   // "%" | "$"
    pub year: String,   // most recent year (macro) or date (market)
    pub value: f64,     // most recent value
    pub previous: Option<f64>,
    pub history: Vec<f64>, // oldest -> newest, for a sparkline
    pub category: String,  // "macro" | "market"
}
