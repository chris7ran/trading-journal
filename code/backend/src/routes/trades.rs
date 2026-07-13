//! CRUD handlers for `/trades`.

use axum::extract::{Path, Query, State};
use axum::Json;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{NewTrade, Trade, TradeFilters, UpdateTrade};
use crate::state::AppState;

const DEFAULT_ACCOUNT: &str = "default";

/// `GET /trades` — list trades with optional filters, newest first.
pub async fn list_trades(
    State(state): State<AppState>,
    Query(filters): Query<TradeFilters>,
) -> AppResult<Json<Vec<Trade>>> {
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT * FROM trades WHERE 1 = 1");

    if let Some(account_id) = &filters.account_id {
        qb.push(" AND account_id = ").push_bind(account_id.clone());
    }
    if let Some(symbol) = &filters.symbol {
        qb.push(" AND symbol = ").push_bind(symbol.clone());
    }
    if let Some(direction) = &filters.direction {
        qb.push(" AND direction = ").push_bind(direction.clone());
    }
    if let Some(from) = &filters.from {
        qb.push(" AND COALESCE(open_time, created_at) >= ")
            .push_bind(from.clone());
    }
    if let Some(to) = &filters.to {
        qb.push(" AND COALESCE(open_time, created_at) <= ")
            .push_bind(to.clone());
    }

    qb.push(" ORDER BY COALESCE(open_time, created_at) DESC ");

    let limit = filters.limit.unwrap_or(100).clamp(1, 1000);
    let offset = filters.offset.unwrap_or(0).max(0);
    qb.push(" LIMIT ").push_bind(limit);
    qb.push(" OFFSET ").push_bind(offset);

    let trades = qb
        .build_query_as::<Trade>()
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(trades))
}

/// `GET /trades/:id` — fetch a single trade.
pub async fn get_trade(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<Trade>> {
    let trade = fetch_trade(&state.pool, &id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(trade))
}

/// `POST /trades` — create a trade (manual entry).
pub async fn create_trade(
    State(state): State<AppState>,
    Json(input): Json<NewTrade>,
) -> AppResult<(axum::http::StatusCode, Json<Trade>)> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::BadRequest("symbol is required".into()));
    }

    let id = Uuid::new_v4().to_string();
    let account_id = input
        .account_id
        .unwrap_or_else(|| DEFAULT_ACCOUNT.to_string());

    let result = sqlx::query(
        r#"
        INSERT INTO trades (
            id, account_id, symbol, direction, open_time, close_time,
            open_price, close_price, lot_size, pnl, pnl_pct, commission, swap,
            setup_tag, emotion_tag, notes, screenshot_url, mt5_ticket,
            followed_plan, respected_sl, pattern_valid, thesis_worked, good_exit
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&account_id)
    .bind(&input.symbol)
    .bind(&input.direction)
    .bind(&input.open_time)
    .bind(&input.close_time)
    .bind(input.open_price)
    .bind(input.close_price)
    .bind(input.lot_size)
    .bind(input.pnl)
    .bind(input.pnl_pct)
    .bind(input.commission)
    .bind(input.swap)
    .bind(&input.setup_tag)
    .bind(&input.emotion_tag)
    .bind(&input.notes)
    .bind(&input.screenshot_url)
    .bind(&input.mt5_ticket)
    .bind(input.followed_plan)
    .bind(input.respected_sl)
    .bind(input.pattern_valid)
    .bind(input.thesis_worked)
    .bind(input.good_exit)
    .execute(&state.pool)
    .await;

    if let Err(e) = result {
        return Err(map_insert_error(e));
    }

    let trade = fetch_trade(&state.pool, &id)
        .await?
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("trade vanished after insert")))?;

    Ok((axum::http::StatusCode::CREATED, Json(trade)))
}

/// `PUT /trades/:id` — partial update (only provided fields change).
pub async fn update_trade(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateTrade>,
) -> AppResult<Json<Trade>> {
    // Ensure it exists first for a clean 404.
    if fetch_trade(&state.pool, &id).await?.is_none() {
        return Err(AppError::NotFound);
    }

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("UPDATE trades SET ");
    let mut first = true;

    // Push `col = ?` for each provided field, inserting commas manually so we
    // never hold a `Separated` borrow across the later `WHERE` push.
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

    set_field!("symbol", patch.symbol);
    set_field!("direction", patch.direction);
    set_field!("open_time", patch.open_time);
    set_field!("close_time", patch.close_time);
    set_field!("open_price", patch.open_price);
    set_field!("close_price", patch.close_price);
    set_field!("lot_size", patch.lot_size);
    set_field!("pnl", patch.pnl);
    set_field!("pnl_pct", patch.pnl_pct);
    set_field!("commission", patch.commission);
    set_field!("swap", patch.swap);
    set_field!("setup_tag", patch.setup_tag);
    set_field!("emotion_tag", patch.emotion_tag);
    set_field!("notes", patch.notes);
    set_field!("screenshot_url", patch.screenshot_url);
    set_field!("followed_plan", patch.followed_plan);
    set_field!("respected_sl", patch.respected_sl);
    set_field!("pattern_valid", patch.pattern_valid);
    set_field!("thesis_worked", patch.thesis_worked);
    set_field!("good_exit", patch.good_exit);

    if !first {
        qb.push(" WHERE id = ").push_bind(id.clone());
        qb.build().execute(&state.pool).await?;
    }

    let trade = fetch_trade(&state.pool, &id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(trade))
}

/// Shared helper: load a trade by id.
async fn fetch_trade(pool: &SqlitePool, id: &str) -> AppResult<Option<Trade>> {
    let trade = sqlx::query_as::<_, Trade>("SELECT * FROM trades WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(trade)
}

/// Translate a unique-constraint violation (duplicate mt5_ticket) into 409.
fn map_insert_error(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db_err) = &e {
        if db_err.is_unique_violation() {
            return AppError::Conflict("a trade with this mt5_ticket already exists".into());
        }
    }
    AppError::Database(e)
}
