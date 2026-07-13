//! In-process integration tests: build the real router against an in-memory
//! SQLite DB and exercise auth + CRUD + CSV import end to end.

use std::str::FromStr;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt; // for `oneshot`

use trading_journal_api::auth::password::hash_password;
use trading_journal_api::config::Config;
use trading_journal_api::routes::build_router;
use trading_journal_api::state::AppState;

const TEST_PASSWORD: &str = "correct horse battery staple";

async fn test_app() -> axum::Router {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);

    // Single connection so the in-memory DB is shared across queries.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect in-memory db");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");

    let config = Config {
        database_url: "sqlite::memory:".into(),
        jwt_secret: "test-secret-key".into(),
        admin_password_hash: hash_password(TEST_PASSWORD).unwrap(),
        bind_addr: "127.0.0.1:0".into(),
        jwt_ttl_hours: 1,
        cors_allowed_origins: "*".into(),
    };

    build_router(AppState::new(pool, config))
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn login(app: &axum::Router) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "password": TEST_PASSWORD }).to_string()))
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let app = test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "password": "wrong" }).to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn trades_require_auth() {
    let app = test_app().await;
    let req = Request::builder()
        .uri("/trades")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_then_list_and_get_trade() {
    let app = test_app().await;
    let token = login(&app).await;

    // Create
    let create = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "symbol": "GER40",
                "direction": "LONG",
                "open_price": 18250.0,
                "close_price": 18300.0,
                "pnl": 500.0,
                "mt5_ticket": "T-1"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let created = body_json(res).await;
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["symbol"], "GER40");

    // Get by id
    let get = Request::builder()
        .uri(format!("/trades/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(get).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // List
    let list = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(list).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let arr = body_json(res).await;
    assert_eq!(arr.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn duplicate_ticket_conflicts() {
    let app = test_app().await;
    let token = login(&app).await;

    let make = || {
        Request::builder()
            .method("POST")
            .uri("/trades")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({ "symbol": "EURUSD", "mt5_ticket": "DUP-1" }).to_string(),
            ))
            .unwrap()
    };

    let first = app.clone().oneshot(make()).await.unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    let second = app.clone().oneshot(make()).await.unwrap();
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn import_csv_dedupes_on_ticket() {
    let app = test_app().await;
    let token = login(&app).await;

    let csv = "\
Ticket,Symbol,Type,Volume,Open Time,Open Price,Close Time,Close Price,Commission,Swap,Profit
501,EURUSD,Buy,0.10,2026.06.20 09:30:00,1.0850,2026.06.20 11:00:00,1.0900,-0.50,0.0,50.00
502,GER40,Sell,1.00,2026.06.20 10:00:00,18250.0,2026.06.20 10:45:00,18200.0,-1.00,0.0,500.00";

    let import = |csv: &'static str| {
        Request::builder()
            .method("POST")
            .uri("/trades/import/csv")
            .header("content-type", "text/csv")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(csv))
            .unwrap()
    };

    let res = app.clone().oneshot(import(csv)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let summary = body_json(res).await;
    assert_eq!(summary["imported"], 2);
    assert_eq!(summary["skipped_duplicates"], 0);

    // Re-importing the same file should skip both as duplicates.
    let res = app.clone().oneshot(import(csv)).await.unwrap();
    let summary = body_json(res).await;
    assert_eq!(summary["imported"], 0);
    assert_eq!(summary["skipped_duplicates"], 2);
}

#[tokio::test]
async fn accounts_create_list_and_rules() {
    let app = test_app().await;
    let token = login(&app).await;

    // Create an account.
    let create = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "FTMO 100k", "balance": 100000.0 }).to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let account = body_json(res).await;
    let account_id = account["id"].as_str().unwrap().to_string();
    assert_eq!(account["broker"], "FusionMarkets"); // default applied

    // List: seeded "default" account + the new one.
    let list = Request::builder()
        .uri("/accounts")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(list).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await.as_array().unwrap().len(), 2);

    // No rules yet -> 404.
    let get_rules = || {
        Request::builder()
            .uri(format!("/accounts/{account_id}/rules"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    let res = app.clone().oneshot(get_rules()).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Upsert rules.
    let put = Request::builder()
        .method("PUT")
        .uri(format!("/accounts/{account_id}/rules"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "daily_drawdown_max": 0.05, "global_drawdown_max": 0.10, "profit_target": 0.10 })
                .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(put).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let rule = body_json(res).await;
    assert_eq!(rule["daily_drawdown_max"], 0.05);
    assert_eq!(rule["consistency_rule_pct"], 0.20); // default

    // Now rules exist.
    let res = app.clone().oneshot(get_rules()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn trade_stats_aggregates_correctly() {
    let app = test_app().await;
    let token = login(&app).await;

    let create_trade = |symbol: &'static str, pnl: f64| {
        Request::builder()
            .method("POST")
            .uri("/trades")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({ "symbol": symbol, "pnl": pnl }).to_string(),
            ))
            .unwrap()
    };

    app.clone().oneshot(create_trade("GER40", 100.0)).await.unwrap();
    app.clone().oneshot(create_trade("GER40", -40.0)).await.unwrap();

    let stats = Request::builder()
        .uri("/trades/stats")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(stats).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let s = body_json(res).await;

    assert_eq!(s["total_trades"], 2);
    assert_eq!(s["wins"], 1);
    assert_eq!(s["losses"], 1);
    assert_eq!(s["win_rate"], 0.5);
    assert_eq!(s["total_pnl"], 60.0);
    assert_eq!(s["profit_factor"], 2.5); // 100 / |−40|
    assert_eq!(s["by_symbol"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn trade_stats_handles_wins_only() {
    // Regression: with no losing trades, gross_loss must still decode as f64
    // (SQLite would otherwise return an INTEGER 0). CAST(... AS REAL) fixes it.
    let app = test_app().await;
    let token = login(&app).await;

    let create = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(json!({ "symbol": "GER40", "pnl": 250.0 }).to_string()))
        .unwrap();
    assert_eq!(app.clone().oneshot(create).await.unwrap().status(), StatusCode::CREATED);

    let stats = Request::builder()
        .uri("/trades/stats")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(stats).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK); // no 500
    let s = body_json(res).await;
    assert_eq!(s["losses"], 0);
    assert_eq!(s["gross_loss"], 0.0);
    assert!(s["profit_factor"].is_null()); // no losses -> undefined
}
