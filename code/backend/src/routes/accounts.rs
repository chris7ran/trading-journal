//! Handlers for `/accounts` and `/accounts/:id/rules`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{Account, NewAccount, PropRule, UpdateAccount, UpsertPropRule};
use crate::state::AppState;

/// `GET /accounts` — list all trading accounts, newest first.
pub async fn list_accounts(State(state): State<AppState>) -> AppResult<Json<Vec<Account>>> {
    let accounts = sqlx::query_as::<_, Account>("SELECT * FROM accounts ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(accounts))
}

/// `POST /accounts` — create a trading account.
pub async fn create_account(
    State(state): State<AppState>,
    Json(input): Json<NewAccount>,
) -> AppResult<(StatusCode, Json<Account>)> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }

    let mt5_login = clean_mt5_login(input.mt5_login.as_deref())?;
    let mt5_server = blank_to_none(input.mt5_server.as_deref());

    let id = Uuid::new_v4().to_string();
    let result = sqlx::query(
        r#"
        INSERT INTO accounts (id, name, broker, balance, currency, is_funded, mt5_login, mt5_server)
        VALUES (?, ?, COALESCE(?, 'FusionMarkets'), ?, COALESCE(?, 'USD'), COALESCE(?, 1), ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.broker)
    .bind(input.balance)
    .bind(&input.currency)
    .bind(input.is_funded)
    .bind(&mt5_login)
    .bind(&mt5_server)
    .execute(&state.pool)
    .await;

    if let Err(e) = result {
        return Err(map_account_error(e));
    }

    let account = fetch_account(&state.pool, &id)
        .await?
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("account vanished after insert")))?;

    Ok((StatusCode::CREATED, Json(account)))
}

/// `PUT /accounts/:id` — partial update (name, balance, currency, is_funded).
pub async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateAccount>,
) -> AppResult<Json<Account>> {
    if fetch_account(&state.pool, &id).await?.is_none() {
        return Err(AppError::NotFound);
    }

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("UPDATE accounts SET ");
    let mut first = true;

    macro_rules! set_field {
        ($col:literal, $val:expr) => {
            if let Some(v) = $val {
                if !first {
                    qb.push(", ");
                }
                qb.push(concat!($col, " = ")).push_bind(v);
                first = false;
            }
        };
    }

    set_field!("name", patch.name);
    set_field!("balance", patch.balance);
    set_field!("currency", patch.currency);
    set_field!("is_funded", patch.is_funded);
    // The two MT5 fields are written by hand rather than through `set_field!`,
    // because they need a distinction the macro cannot express: omitting the
    // field leaves the current value alone, while sending an empty string
    // clears it. Without that, an account bound to the wrong terminal could
    // never be unbound except by editing the database.
    if let Some(raw) = patch.mt5_login.as_deref() {
        if !first {
            qb.push(", ");
        }
        qb.push("mt5_login = ")
            .push_bind(clean_mt5_login(Some(raw))?);
        first = false;
    }
    if let Some(raw) = patch.mt5_server.as_deref() {
        if !first {
            qb.push(", ");
        }
        qb.push("mt5_server = ").push_bind(blank_to_none(Some(raw)));
        first = false;
    }

    if !first {
        qb.push(" WHERE id = ").push_bind(id.clone());
        if let Err(e) = qb.build().execute(&state.pool).await {
            return Err(map_account_error(e));
        }
    }

    let account = fetch_account(&state.pool, &id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(account))
}

/// `GET /accounts/:id/rules` — fetch prop firm rules for an account.
pub async fn get_rules(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> AppResult<Json<PropRule>> {
    if fetch_account(&state.pool, &account_id).await?.is_none() {
        return Err(AppError::NotFound);
    }

    let rule = sqlx::query_as::<_, PropRule>("SELECT * FROM prop_rules WHERE account_id = ?")
        .bind(&account_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)?;

    Ok(Json(rule))
}

/// `PUT /accounts/:id/rules` — insert-or-update the rules for an account.
pub async fn upsert_rules(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Json(input): Json<UpsertPropRule>,
) -> AppResult<Json<PropRule>> {
    if fetch_account(&state.pool, &account_id).await?.is_none() {
        return Err(AppError::NotFound);
    }

    let consistency = input.consistency_rule_pct.unwrap_or(0.20);

    // One rule row per account: use the account_id as a natural conflict key.
    // We keep a separate surrogate `id`, generated only on first insert.
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM prop_rules WHERE account_id = ?")
            .bind(&account_id)
            .fetch_optional(&state.pool)
            .await?;

    let id = existing
        .map(|(id,)| id)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    sqlx::query(
        r#"
        INSERT INTO prop_rules (
            id, account_id, daily_drawdown_max, global_drawdown_max,
            profit_target, min_trading_days, consistency_rule_pct, lot_size_max
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            daily_drawdown_max   = excluded.daily_drawdown_max,
            global_drawdown_max  = excluded.global_drawdown_max,
            profit_target        = excluded.profit_target,
            min_trading_days     = excluded.min_trading_days,
            consistency_rule_pct = excluded.consistency_rule_pct,
            lot_size_max         = excluded.lot_size_max
        "#,
    )
    .bind(&id)
    .bind(&account_id)
    .bind(input.daily_drawdown_max)
    .bind(input.global_drawdown_max)
    .bind(input.profit_target)
    .bind(input.min_trading_days)
    .bind(consistency)
    .bind(input.lot_size_max)
    .execute(&state.pool)
    .await?;

    let rule = sqlx::query_as::<_, PropRule>("SELECT * FROM prop_rules WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(rule))
}

/// `DELETE /accounts/:id` — remove the account and, in cascade, its trades and
/// prop rules. 404 if it doesn't exist.
///
/// The delete is a single statement: the schema's `ON DELETE CASCADE` (with the
/// `foreign_keys` pragma on for every pooled connection) drops the account's
/// trades, prop_rules and sessions atomically — no explicit transaction needed.
pub async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    let res = sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_account(pool: &SqlitePool, id: &str) -> AppResult<Option<Account>> {
    let account = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(account)
}

/// Longest MT5 login we accept. Real ones are 8-9 digits; the ceiling is there
/// to keep a pasted paragraph out of the column, not to validate a format.
const MAX_MT5_LOGIN_LEN: usize = 20;

/// Normalise an MT5 login: trim, treat blank as "no terminal", keep digits only.
///
/// Digits-only is not cosmetic. The login is concatenated into the stored
/// ticket (`<login>:<ticket>`) which carries a uniqueness constraint, so a
/// login containing a colon would let two different terminals produce the same
/// stored ticket and silently overwrite each other's trades.
fn clean_mt5_login(raw: Option<&str>) -> AppResult<Option<String>> {
    let Some(value) = blank_to_none(raw) else {
        return Ok(None);
    };
    if value.len() > MAX_MT5_LOGIN_LEN {
        return Err(AppError::BadRequest(format!(
            "mt5_login too long (max {MAX_MT5_LOGIN_LEN} characters)"
        )));
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::BadRequest(
            "mt5_login must contain digits only".into(),
        ));
    }
    Ok(Some(value))
}

/// Trim, and map an empty result to `None` so blank input stores SQL NULL
/// rather than an empty string — the partial unique index on `mt5_login`
/// ignores NULLs but would treat several empty strings as duplicates.
fn blank_to_none(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Translate the `mt5_login` unique-index violation into a 409 the caller can
/// act on, instead of an opaque 500.
fn map_account_error(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db_err) = &e {
        if db_err.is_unique_violation() {
            return AppError::Conflict(
                "un autre compte déclare déjà ce login MT5".into(),
            );
        }
    }
    AppError::Database(e)
}
