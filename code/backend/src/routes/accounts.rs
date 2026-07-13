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

    let id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO accounts (id, name, broker, balance, currency, is_funded)
        VALUES (?, ?, COALESCE(?, 'FusionMarkets'), ?, COALESCE(?, 'USD'), COALESCE(?, 1))
        "#,
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.broker)
    .bind(input.balance)
    .bind(&input.currency)
    .bind(input.is_funded)
    .execute(&state.pool)
    .await?;

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

    if !first {
        qb.push(" WHERE id = ").push_bind(id.clone());
        qb.build().execute(&state.pool).await?;
    }

    let account = fetch_account(&state.pool, &id).await?.ok_or(AppError::NotFound)?;
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

async fn fetch_account(pool: &SqlitePool, id: &str) -> AppResult<Option<Account>> {
    let account = sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(account)
}
