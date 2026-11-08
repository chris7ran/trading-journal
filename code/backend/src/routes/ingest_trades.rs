//! Trade ingestion from the MT5 side.
//!
//! Sits next to the candle feed on the machine router, behind the same
//! `X-Ingest-Token`. What the token grants widens here — it can now write
//! trades, not just prices — so the handler is deliberately narrow about what
//! that means:
//!
//! * The caller never names a journal account. It states the MT5 login it is
//!   connected to, and the server resolves the account from
//!   `accounts.mt5_login`. An account that declares no login is unreachable
//!   through this endpoint, which is what keeps the imported Scale FX history
//!   and every hand-entered trade out of reach.
//! * Stored tickets are prefixed with the login (`100000001:21455031`). Ticket
//!   numbers restart per account at some brokers, so the bare number is not
//!   unique across terminals; the prefix makes the existing uniqueness
//!   constraint on `mt5_ticket` meaningful again, and it bounds an upsert to
//!   rows the same terminal created.
//! * The upsert refreshes only what the broker owns — prices, volumes, P&L,
//!   times. Everything the trader wrote (setup, emotion, notes, screenshot,
//!   the five review flags) is left untouched, so a replay after a crash can
//!   never erase a review.
//!
//! Replaying a batch is therefore safe and expected: the execution layer
//! pushes its open positions repeatedly and the same rows simply converge.

use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::routes::ingest::{clean_symbol, parse_utc};
use crate::state::AppState;

/// Upper bound on a single batch. A trading day produces a handful of trades;
/// this only exists so a malformed loop cannot push an unbounded payload.
const MAX_BATCH: usize = 500;

/// Longest MT5 ticket we accept (uint64 maxes out at 20 digits).
const MAX_TICKET_LEN: usize = 20;

/// Longest plan identifier. Plans are UUIDs today; the ceiling is slack.
const MAX_PLAN_ID_LEN: usize = 64;

/// Tolerance for clock skew between the MT5 server and ours.
const MAX_FUTURE_SKEW_MINUTES: i64 = 30;

// --- Wire types -------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TradeIngestPayload {
    /// MT5 account number of the terminal sending this batch.
    pub login: String,
    pub trades: Vec<TradeIn>,
}

#[derive(Debug, Deserialize)]
pub struct TradeIn {
    /// MT5 position ticket, as a decimal string. JSON numbers lose precision
    /// above 2^53 and MT5 tickets are uint64, so this stays a string all the
    /// way from the terminal to the database.
    pub ticket: String,
    pub symbol: String,
    /// `LONG`/`SHORT`, or MT5's own `BUY`/`SELL` — both accepted, stored as
    /// `LONG`/`SHORT` to match what the rest of the journal already holds.
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub open_time: Option<String>,
    /// Absent while the position is still open.
    #[serde(default)]
    pub close_time: Option<String>,
    #[serde(default)]
    pub open_price: Option<f64>,
    #[serde(default)]
    pub close_price: Option<f64>,
    #[serde(default)]
    pub stop_loss: Option<f64>,
    #[serde(default)]
    pub take_profit: Option<f64>,
    #[serde(default)]
    pub lot_size: Option<f64>,
    #[serde(default)]
    pub pnl: Option<f64>,
    #[serde(default)]
    pub pnl_pct: Option<f64>,
    #[serde(default)]
    pub commission: Option<f64>,
    #[serde(default)]
    pub swap: Option<f64>,
    /// Ties the legs of one trade plan together. Omitted for anything the
    /// trader opened by hand in the terminal.
    #[serde(default)]
    pub plan_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TradeIngestResult {
    pub account_id: String,
    pub received: usize,
    pub created: usize,
    pub updated: usize,
    /// Rows that resolved to a different account and were left alone. Always
    /// zero in practice; a non-zero value means an account's `mt5_login` was
    /// reassigned after trades had already been stored under it.
    pub skipped: usize,
}

// --- Handler ----------------------------------------------------------------

/// `POST /ingest/trades` — insert or refresh a batch of MT5 trades.
pub async fn ingest_trades(
    State(state): State<AppState>,
    Json(payload): Json<TradeIngestPayload>,
) -> AppResult<Json<TradeIngestResult>> {
    let login = clean_login(&payload.login)?;

    // Resolve the account before validating the batch: a wrong login is the
    // likeliest mistake and deserves the clearest error.
    let account_id: String = sqlx::query_scalar("SELECT id FROM accounts WHERE mt5_login = ?")
        .bind(&login)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "aucun compte du journal ne déclare le login MT5 {login}"
            ))
        })?;

    if payload.trades.len() > MAX_BATCH {
        return Err(AppError::BadRequest(format!(
            "batch too large: {} trades (max {MAX_BATCH})",
            payload.trades.len()
        )));
    }

    // Validate everything before writing anything: a half-applied batch would
    // leave the sender unable to tell what still needs pushing.
    let mut rows = Vec::with_capacity(payload.trades.len());
    for (i, trade) in payload.trades.iter().enumerate() {
        rows.push(
            prepare(trade, &login).map_err(|e| AppError::BadRequest(format!("trade {i}: {e}")))?,
        );
    }

    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;

    let mut tx = state.pool.begin().await?;
    for row in &rows {
        // Knowing whether this is a first sighting or a refresh costs one
        // indexed lookup and turns the response into something an operator can
        // read: "created 3, updated 12" says the feed is healthy, "created 15"
        // every cycle says the ticket prefix is not stable.
        let existing: Option<String> =
            sqlx::query_scalar("SELECT account_id FROM trades WHERE mt5_ticket = ?")
                .bind(&row.mt5_ticket)
                .fetch_optional(&mut *tx)
                .await?;

        match existing.as_deref() {
            Some(owner) if owner != account_id => {
                skipped += 1;
                continue;
            }
            Some(_) => updated += 1,
            None => created += 1,
        }

        sqlx::query(
            r#"
            INSERT INTO trades (
                id, account_id, symbol, direction, open_time, close_time,
                open_price, close_price, stop_loss, take_profit, lot_size,
                pnl, pnl_pct, commission, swap, mt5_ticket, plan_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(mt5_ticket) DO UPDATE SET
                -- COALESCE throughout: an omitted field means "I don't know",
                -- never "it is empty". MT5 reports a closed position without
                -- the stop it was opened with, and a terminal restarted
                -- mid-session has lost its plan state — assigning those
                -- straight across would quietly erase the entry stop that
                -- every R multiple is computed from, and orphan a leg from
                -- its siblings. A value actually sent still overwrites, which
                -- is what lets the watcher move a stop to break-even.
                symbol      = COALESCE(excluded.symbol,      trades.symbol),
                direction   = COALESCE(excluded.direction,   trades.direction),
                open_time   = COALESCE(excluded.open_time,   trades.open_time),
                close_time  = COALESCE(excluded.close_time,  trades.close_time),
                open_price  = COALESCE(excluded.open_price,  trades.open_price),
                close_price = COALESCE(excluded.close_price, trades.close_price),
                stop_loss   = COALESCE(excluded.stop_loss,   trades.stop_loss),
                take_profit = COALESCE(excluded.take_profit, trades.take_profit),
                lot_size    = COALESCE(excluded.lot_size,    trades.lot_size),
                pnl         = COALESCE(excluded.pnl,         trades.pnl),
                pnl_pct     = COALESCE(excluded.pnl_pct,     trades.pnl_pct),
                commission  = COALESCE(excluded.commission,  trades.commission),
                swap        = COALESCE(excluded.swap,        trades.swap),
                plan_id     = COALESCE(excluded.plan_id,     trades.plan_id)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&account_id)
        .bind(&row.symbol)
        .bind(&row.direction)
        .bind(&row.open_time)
        .bind(&row.close_time)
        .bind(row.open_price)
        .bind(row.close_price)
        .bind(row.stop_loss)
        .bind(row.take_profit)
        .bind(row.lot_size)
        .bind(row.pnl)
        .bind(row.pnl_pct)
        .bind(row.commission)
        .bind(row.swap)
        .bind(&row.mt5_ticket)
        .bind(&row.plan_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    if skipped > 0 {
        tracing::warn!(
            login = %login,
            skipped,
            "trades ingérés ignorés : tickets déjà rattachés à un autre compte"
        );
    }
    tracing::info!(login = %login, created, updated, "ingested trades");

    Ok(Json(TradeIngestResult {
        account_id,
        received: payload.trades.len(),
        created,
        updated,
        skipped,
    }))
}

// --- Validation -------------------------------------------------------------

/// A validated row, ready to bind.
struct Prepared {
    symbol: String,
    direction: Option<String>,
    open_time: Option<String>,
    close_time: Option<String>,
    open_price: Option<f64>,
    close_price: Option<f64>,
    stop_loss: Option<f64>,
    take_profit: Option<f64>,
    lot_size: Option<f64>,
    pnl: Option<f64>,
    pnl_pct: Option<f64>,
    commission: Option<f64>,
    swap: Option<f64>,
    mt5_ticket: String,
    plan_id: Option<String>,
}

fn prepare(input: &TradeIn, login: &str) -> Result<Prepared, String> {
    let ticket = clean_ticket(&input.ticket)?;
    let symbol = clean_symbol(&input.symbol).map_err(|e| e.to_string())?;
    let direction = normalise_direction(input.direction.as_deref())?;

    let open_time = clean_time(input.open_time.as_deref(), "open_time")?;
    let close_time = clean_time(input.close_time.as_deref(), "close_time")?;

    for (name, value) in [
        ("open_price", input.open_price),
        ("close_price", input.close_price),
        ("stop_loss", input.stop_loss),
        ("take_profit", input.take_profit),
    ] {
        if let Some(v) = value {
            if !v.is_finite() || v <= 0.0 {
                return Err(format!("{name} must be a positive finite number"));
            }
        }
    }
    if let Some(v) = input.lot_size {
        if !v.is_finite() || v <= 0.0 {
            return Err("lot_size must be a positive finite number".into());
        }
    }
    // P&L, commission and swap are signed: only finiteness matters. A NaN here
    // would propagate silently through every aggregate in the stats endpoint.
    for (name, value) in [
        ("pnl", input.pnl),
        ("pnl_pct", input.pnl_pct),
        ("commission", input.commission),
        ("swap", input.swap),
    ] {
        if let Some(v) = value {
            if !v.is_finite() {
                return Err(format!("{name} is not a finite number"));
            }
        }
    }

    let plan_id = match input.plan_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(v) if v.len() > MAX_PLAN_ID_LEN => {
            return Err(format!("plan_id too long (max {MAX_PLAN_ID_LEN})"))
        }
        Some(v) => Some(v.to_owned()),
    };

    Ok(Prepared {
        symbol,
        direction,
        open_time,
        close_time,
        open_price: input.open_price,
        close_price: input.close_price,
        stop_loss: input.stop_loss,
        take_profit: input.take_profit,
        lot_size: input.lot_size,
        pnl: input.pnl,
        pnl_pct: input.pnl_pct,
        commission: input.commission,
        swap: input.swap,
        mt5_ticket: stored_ticket(login, &ticket),
        plan_id,
    })
}

/// The key a trade from a bound terminal is stored under.
///
/// Shared with the CSV importer on purpose. Both paths can describe the same
/// trade — the execution layer pushes it live, and the trader may later import
/// the broker's own history export of the same day — and if the two disagreed
/// on the key, one trade would become two rows and every statistic would
/// double-count it.
pub(crate) fn stored_ticket(login: &str, ticket: &str) -> String {
    format!("{login}:{}", normalise_ticket(ticket))
}

/// Strip leading zeros so `0000021455031` and `21455031` are one trade, while
/// keeping a lone `0` representable.
fn normalise_ticket(ticket: &str) -> &str {
    let trimmed = ticket.trim().trim_start_matches('0');
    if trimmed.is_empty() {
        "0"
    } else {
        trimmed
    }
}

fn clean_login(raw: &str) -> AppResult<String> {
    let login = raw.trim();
    if login.is_empty() {
        return Err(AppError::BadRequest("login is required".into()));
    }
    // Digits only, for the same reason the accounts route enforces it: the
    // login becomes the prefix of a unique key, and a separator smuggled into
    // it would let one terminal address another's rows.
    if login.len() > MAX_TICKET_LEN || !login.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::BadRequest(
            "login must be a decimal MT5 account number".into(),
        ));
    }
    Ok(login.to_owned())
}

fn clean_ticket(raw: &str) -> Result<String, String> {
    let ticket = raw.trim();
    if ticket.is_empty() {
        return Err("ticket is required".into());
    }
    if ticket.len() > MAX_TICKET_LEN || !ticket.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("ticket '{ticket}' is not a decimal MT5 ticket"));
    }
    Ok(ticket.to_owned())
}

/// Accept the journal's own vocabulary and MT5's, store the journal's.
fn normalise_direction(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match value.to_ascii_uppercase().as_str() {
        "LONG" | "BUY" => Ok(Some("LONG".into())),
        "SHORT" | "SELL" => Ok(Some("SHORT".into())),
        other => Err(format!("unknown direction '{other}'")),
    }
}

/// Parse a timestamp, reject a clock far in the future, and re-emit it in the
/// exact shape the rest of the journal stores — the archive groups trades with
/// `substr(open_time, 1, 10)`, so a stray offset suffix would file a trade on
/// the wrong day.
fn clean_time(raw: Option<&str>, field: &str) -> Result<Option<String>, String> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let ts = parse_utc(value).map_err(|e| format!("{field}: {e}"))?;
    if ts > Utc::now() + Duration::minutes(MAX_FUTURE_SKEW_MINUTES) {
        return Err(format!(
            "{field}: {value} is in the future — check the terminal is sending UTC"
        ));
    }
    Ok(Some(ts.format("%Y-%m-%dT%H:%M:%S").to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(ticket: &str) -> TradeIn {
        TradeIn {
            ticket: ticket.into(),
            symbol: "GER40Cash#".into(),
            direction: Some("buy".into()),
            open_time: Some("2026-11-06 08:15:00".into()),
            close_time: None,
            open_price: Some(26_300.0),
            close_price: None,
            stop_loss: Some(26_250.0),
            take_profit: Some(26_400.0),
            lot_size: Some(6.7),
            pnl: Some(-12.5),
            pnl_pct: None,
            commission: Some(0.0),
            swap: Some(0.0),
            plan_id: Some("plan-1".into()),
        }
    }

    #[test]
    fn prefixes_the_ticket_with_the_login() {
        let row = prepare(&trade("21455031"), "100000001").unwrap();
        assert_eq!(row.mt5_ticket, "100000001:21455031");
    }

    #[test]
    fn the_same_ticket_on_two_terminals_stays_distinct() {
        let a = prepare(&trade("5001"), "100000001").unwrap();
        let b = prepare(&trade("5001"), "100000002").unwrap();
        assert_ne!(a.mt5_ticket, b.mt5_ticket);
    }

    #[test]
    fn normalises_leading_zeros_so_one_trade_is_one_row() {
        let padded = prepare(&trade("0000021455031"), "1").unwrap();
        let plain = prepare(&trade("21455031"), "1").unwrap();
        assert_eq!(padded.mt5_ticket, plain.mt5_ticket);
    }

    #[test]
    fn translates_mt5_sides_to_the_journal_vocabulary() {
        assert_eq!(normalise_direction(Some("buy")).unwrap().unwrap(), "LONG");
        assert_eq!(normalise_direction(Some("SELL")).unwrap().unwrap(), "SHORT");
        assert_eq!(normalise_direction(Some("Long")).unwrap().unwrap(), "LONG");
        assert!(normalise_direction(Some("sideways")).is_err());
        assert!(normalise_direction(Some("  ")).unwrap().is_none());
    }

    #[test]
    fn stores_times_without_a_zone_suffix() {
        let row = prepare(&trade("1"), "1").unwrap();
        assert_eq!(row.open_time.unwrap(), "2026-11-06T08:15:00");
    }

    #[test]
    fn rejects_a_ticket_that_is_not_a_number() {
        let mut t = trade("1");
        t.ticket = "21455031; DROP".into();
        assert!(prepare(&t, "1").is_err());
    }

    #[test]
    fn rejects_a_login_that_could_forge_another_terminals_key() {
        assert!(clean_login("100000001:9").is_err());
        assert!(clean_login("").is_err());
        assert!(clean_login("100000001").is_ok());
    }

    #[test]
    fn rejects_non_finite_money() {
        let mut t = trade("1");
        t.pnl = Some(f64::NAN);
        assert!(prepare(&t, "1").is_err());
    }

    #[test]
    fn rejects_a_negative_price() {
        let mut t = trade("1");
        t.open_price = Some(-1.0);
        assert!(prepare(&t, "1").is_err());
    }

    #[test]
    fn rejects_a_close_time_in_the_future() {
        let mut t = trade("1");
        t.close_time = Some((Utc::now() + Duration::days(2)).to_rfc3339());
        assert!(prepare(&t, "1").is_err());
    }

    #[test]
    fn accepts_an_open_position_with_no_close() {
        let row = prepare(&trade("1"), "1").unwrap();
        assert!(row.close_time.is_none());
        assert!(row.close_price.is_none());
    }
}
