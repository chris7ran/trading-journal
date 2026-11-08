//! In-process integration tests: build the real router against an in-memory
//! SQLite DB and exercise auth + CRUD + CSV import end to end.

use std::net::SocketAddr;
use std::str::FromStr;

use axum::body::Body;
use axum::extract::ConnectInfo;
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
const TEST_INGEST_TOKEN: &str = "test-ingest-token";
const TEST_MCP_TOKEN: &str = "test-mcp-token";

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
        ingest_token: TEST_INGEST_TOKEN.into(),
        mcp_token: TEST_MCP_TOKEN.into(),
    };

    build_router(AppState::new(pool, config))
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

// The rate-limit layer on `/auth/login` keys on the peer IP from
// `ConnectInfo`, which `axum::serve` injects in production but `.oneshot()`
// does not — so tests must insert it manually.
fn login_request(password: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "password": password }).to_string()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
    req
}

async fn login(app: &axum::Router) -> String {
    let res = app
        .clone()
        .oneshot(login_request(TEST_PASSWORD))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await["token"].as_str().unwrap().to_string()
}

// There's no seeded default account, so trade creation needs one to exist.
// Returns the new account id.
async fn seed_account(app: &axum::Router, token: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(json!({ "name": "Test" }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let app = test_app().await;
    let res = app.oneshot(login_request("wrong")).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_rate_limited_after_burst() {
    let app = test_app().await;

    // Burst size is 5: the first 5 attempts reach the handler (and are
    // rejected for a wrong password), the 6th is throttled by the limiter.
    for _ in 0..5 {
        let res = app.clone().oneshot(login_request("wrong")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
    let res = app.clone().oneshot(login_request("wrong")).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
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
    seed_account(&app, &token).await;

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
    seed_account(&app, &token).await;

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

    // Account header lets the import auto-create the target (no default exists).
    let csv = "\
Nom:,,,test-account
Compte:,,,\"900001 (USD, Test)\"
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
async fn import_resolves_account_and_refreshes_fees() {
    let app = test_app().await;
    let token = login(&app).await;

    let import = |csv: &'static str| {
        Request::builder()
            .method("POST")
            .uri("/trades/import/csv")
            .header("content-type", "text/csv")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(csv))
            .unwrap()
    };

    // Report carrying its own account header; no ?account_id given.
    let csv = "\
Nom:,,,tran_christophe-INSTANT
Compte:,,,\"314299306 (USD, GoatFunded-Server, real, Hedge)\"
Positions
Heure,Position,Symbole,Type,Volume,Prix,S / L,T / P,Heure,Prix,Commission,Echange,Profit
2026.06.20 09:30:00,701,US30.x,buy,1,48000,0,0,2026.06.20 11:00:00,48100,0.0,-5.0,100.0";

    let res = app.clone().oneshot(import(csv)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let summary = body_json(res).await;
    assert_eq!(summary["imported"], 1);
    // Account was auto-created from the report's login number.
    let account_id = summary["account_id"].as_str().unwrap().to_string();
    assert_eq!(summary["account_name"], "314299306");

    // Net P&L = 100.0 + 0.0 + (-5.0) = 95.0, on the resolved account.
    let trades = list_trades(&app, &token).await;
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0]["pnl"], 95.0);
    assert_eq!(trades[0]["account_id"], account_id);

    // Re-import the same ticket with a heavier swap: refreshed, not duplicated.
    let csv2 = "\
Nom:,,,tran_christophe-INSTANT
Compte:,,,\"314299306 (USD, GoatFunded-Server, real, Hedge)\"
Positions
Heure,Position,Symbole,Type,Volume,Prix,S / L,T / P,Heure,Prix,Commission,Echange,Profit
2026.06.20 09:30:00,701,US30.x,buy,1,48000,0,0,2026.06.20 11:00:00,48100,0.0,-12.0,100.0";
    let res = app.clone().oneshot(import(csv2)).await.unwrap();
    let summary = body_json(res).await;
    assert_eq!(summary["imported"], 0);
    assert_eq!(summary["skipped_duplicates"], 1);

    let trades = list_trades(&app, &token).await;
    assert_eq!(trades.len(), 1); // still one row
    assert_eq!(trades[0]["pnl"], 88.0); // 100 - 12, fees corrected
}

async fn list_trades(app: &axum::Router, token: &str) -> Vec<Value> {
    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await.as_array().unwrap().clone()
}

async fn list_accounts(app: &axum::Router, token: &str) -> Vec<Value> {
    let req = Request::builder()
        .uri("/accounts")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await.as_array().unwrap().clone()
}

#[tokio::test]
async fn no_seeded_default_account() {
    // The 0004 migration removes the seeded "default" account on a fresh DB.
    let app = test_app().await;
    let token = login(&app).await;
    let accounts = list_accounts(&app, &token).await;
    assert!(
        accounts.is_empty(),
        "expected no accounts, got {accounts:?}"
    );
}

#[tokio::test]
async fn delete_trade_then_404() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_account(&app, &token).await;

    let create = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "symbol": "GER40", "pnl": 10.0 }).to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(create).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = body_json(res).await["id"].as_str().unwrap().to_string();

    let del = || {
        Request::builder()
            .method("DELETE")
            .uri(format!("/trades/{id}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    // First delete succeeds, the trade is gone, a second delete 404s.
    assert_eq!(
        app.clone().oneshot(del()).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert!(list_trades(&app, &token).await.is_empty());
    assert_eq!(
        app.clone().oneshot(del()).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn delete_account_cascades_trades_and_rules() {
    let app = test_app().await;
    let token = login(&app).await;
    let account_id = seed_account(&app, &token).await;

    // A trade in the account...
    let create = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "symbol": "GER40", "pnl": 10.0, "account_id": account_id }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(create).await.unwrap().status(),
        StatusCode::CREATED
    );
    // ...and prop rules for it.
    let put_rules = Request::builder()
        .method("PUT")
        .uri(format!("/accounts/{account_id}/rules"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "daily_drawdown_max": 0.05 }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(put_rules).await.unwrap().status(),
        StatusCode::OK
    );

    // Delete the account: cascade removes its trades and rules.
    let del = Request::builder()
        .method("DELETE")
        .uri(format!("/accounts/{account_id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(del).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );

    assert!(list_accounts(&app, &token).await.is_empty());
    assert!(list_trades(&app, &token).await.is_empty()); // cascaded

    // Rules are gone too (404).
    let get_rules = Request::builder()
        .uri(format!("/accounts/{account_id}/rules"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(get_rules).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
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

    // List: just the one we created (no seeded default account anymore).
    let list = Request::builder()
        .uri("/accounts")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(list).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await.as_array().unwrap().len(), 1);

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
    seed_account(&app, &token).await;

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

    app.clone()
        .oneshot(create_trade("GER40", 100.0))
        .await
        .unwrap();
    app.clone()
        .oneshot(create_trade("GER40", -40.0))
        .await
        .unwrap();

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
    seed_account(&app, &token).await;

    let create = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "symbol": "GER40", "pnl": 250.0 }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(create).await.unwrap().status(),
        StatusCode::CREATED
    );

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

// --- Candle ingestion + daily levels ----------------------------------------

fn ingest_request(token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/ingest/candles")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("x-ingest-token", t);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// Three M5 candles inside the London opening range of 15 July 2026.
/// London is on BST that day, so 08:00 local is 07:00 UTC.
fn london_orb_batch() -> Value {
    json!({
        "symbol": "GER40",
        "timeframe": "M5",
        "candles": [
            { "t": "2026-07-15T07:00:00Z", "o": 100.0, "h": 103.0, "l": 99.0,  "c": 102.0, "v": 10.0 },
            { "t": "2026-07-15T07:05:00Z", "o": 102.0, "h": 105.0, "l": 101.0, "c": 104.0, "v": 12.0 },
            { "t": "2026-07-15T07:10:00Z", "o": 104.0, "h": 104.5, "l": 100.0, "c": 101.0, "v": 9.0  }
        ]
    })
}

#[tokio::test]
async fn ingest_rejects_a_missing_or_wrong_token() {
    let app = test_app().await;

    let res = app
        .clone()
        .oneshot(ingest_request(None, london_orb_batch()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .clone()
        .oneshot(ingest_request(Some("not-the-token"), london_orb_batch()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_does_not_accept_the_user_jwt_as_an_ingest_token() {
    // The two credentials are deliberately separate; a JWT must not open the
    // machine write path, and vice versa.
    let app = test_app().await;
    let jwt = login(&app).await;
    let res = app
        .clone()
        .oneshot(ingest_request(Some(&jwt), london_orb_batch()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingest_is_idempotent_so_the_ea_can_safely_replay() {
    let app = test_app().await;

    for _ in 0..3 {
        let res = app
            .clone()
            .oneshot(ingest_request(Some(TEST_INGEST_TOKEN), london_orb_batch()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["symbol"], "GER40");
        assert_eq!(body["stored"], 3);
    }

    // Three replays, still three candles.
    let req = Request::builder()
        .uri("/ingest/status")
        .header("x-ingest-token", TEST_INGEST_TOKEN)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["symbols"][0]["symbol"], "GER40");
    assert_eq!(body["symbols"][0]["count"], 3);
}

#[tokio::test]
async fn ingest_rejects_an_impossible_candle() {
    let app = test_app().await;
    let bad = json!({
        "symbol": "GER40",
        "candles": [
            { "t": "2026-07-15T07:00:00Z", "o": 100.0, "h": 98.0, "l": 99.0, "c": 99.0 }
        ]
    });
    let res = app
        .clone()
        .oneshot(ingest_request(Some(TEST_INGEST_TOKEN), bad))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_rejects_timestamps_from_the_future() {
    // The single most likely EA bug: sending broker server time (often UTC+2 or
    // UTC+3) instead of UTC. Better to fail loudly than to file candles into
    // the wrong session for months.
    let app = test_app().await;
    let future = (chrono::Utc::now() + chrono::Duration::hours(3))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let bad = json!({
        "symbol": "GER40",
        "candles": [{ "t": future, "o": 100.0, "h": 101.0, "l": 99.0, "c": 100.0 }]
    });
    let res = app
        .clone()
        .oneshot(ingest_request(Some(TEST_INGEST_TOKEN), bad))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn levels_require_auth() {
    let app = test_app().await;
    let req = Request::builder()
        .uri("/levels?symbol=GER40")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn ingested_candles_become_the_london_opening_range() {
    let app = test_app().await;
    let token = login(&app).await;

    let res = app
        .clone()
        .oneshot(ingest_request(Some(TEST_INGEST_TOKEN), london_orb_batch()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let req = Request::builder()
        .uri("/levels?symbol=ger40&date=2026-07-15")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;

    assert_eq!(body["symbol"], "GER40"); // lowercase query normalised
    assert_eq!(body["day"]["high"], 105.0);
    assert_eq!(body["day"]["low"], 99.0);

    let orb = body["opening_ranges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["key"] == "orb_london")
        .unwrap()
        .clone();
    assert_eq!(orb["bars"], 3);
    assert_eq!(orb["high"], 105.0);
    assert_eq!(orb["low"], 99.0);
    assert_eq!(orb["range"], 6.0);
}

#[tokio::test]
async fn levels_for_a_day_without_data_are_empty_not_an_error() {
    let app = test_app().await;
    let token = login(&app).await;
    let req = Request::builder()
        .uri("/levels?symbol=GER40&date=2026-07-15")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert!(body["day"].is_null());
    assert_eq!(body["sessions"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn levels_reject_a_malformed_date() {
    let app = test_app().await;
    let token = login(&app).await;
    let req = Request::builder()
        .uri("/levels?symbol=GER40&date=15-07-2026")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

// --- Morning brief ----------------------------------------------------------

/// One session's worth of M5 candles, 08:00 UTC onwards so it lands squarely
/// inside the Paris calendar day. The whole range is made by the first candle,
/// which keeps the arithmetic in the assertions exact.
fn session_batch(symbol: &str, day: &str, high: f64, low: f64) -> Value {
    let candles: Vec<Value> = (0..12)
        .map(|i| {
            let h = if i == 0 { high } else { low + 0.5 };
            json!({
                "t": format!("{day}T08:{:02}:00Z", i * 5),
                "o": low,
                "h": h,
                "l": low,
                "c": low + 0.5
            })
        })
        .collect();
    json!({ "symbol": symbol, "timeframe": "M5", "candles": candles })
}

async fn seed_candles(app: &axum::Router, batches: Vec<Value>) {
    for batch in batches {
        let res = app
            .clone()
            .oneshot(ingest_request(Some(TEST_INGEST_TOKEN), batch))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}

/// Ten quiet 100-point sessions (20–29 July), then a 500-point day (30 July).
fn quiet_then_violent(symbol: &str) -> Vec<Value> {
    let mut batches: Vec<Value> = (20..=29)
        .map(|day| session_batch(symbol, &format!("2026-07-{day:02}"), 1100.0, 1000.0))
        .collect();
    batches.push(session_batch(symbol, "2026-07-30", 1500.0, 1000.0));
    batches
}

async fn get_brief(app: &axum::Router, token: &str, symbol: &str, date: &str) -> Value {
    let req = Request::builder()
        .uri(format!("/brief?symbol={symbol}&date={date}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

#[tokio::test]
async fn brief_requires_auth() {
    let app = test_app().await;
    let req = Request::builder()
        .uri("/brief?symbol=GER40")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn brief_measures_the_day_against_the_average_daily_range() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    let body = get_brief(&app, &token, "GER40", "2026-07-30").await;

    // Ten completed 100-point sessions -> ADR 100, so today's 500-point range
    // reads as 500% of it. Had today leaked into its own average it would show
    // roughly 145% — which is the whole reason the average excludes it.
    assert_eq!(body["stats"]["sessions"], 10);
    assert_eq!(body["stats"]["adr"], 100.0);
    assert_eq!(body["stats"]["adr_median"], 100.0);
    assert_eq!(body["stats"]["day_range"], 500.0);
    assert_eq!(body["stats"]["day_pct_of_adr"], 500.0);

    // The previous session was an ordinary day.
    assert_eq!(body["stats"]["previous_day_pct_of_adr"], 100.0);

    // Levels ride along, so the screen needs a single request.
    assert_eq!(body["levels"]["day"]["high"], 1500.0);

    // And it says so in words, without telling the reader what to do.
    let observations = body["observations"].as_array().unwrap();
    assert!(observations.iter().any(|o| o["key"] == "day_range"));
}

#[tokio::test]
async fn brief_reads_the_session_against_the_previous_midpoint() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("US30")).await;

    let body = get_brief(&app, &token, "US30", "2026-07-30").await;

    // Previous session ran 1000 -> 1100, so the 0.5 sits at 1050. Today opened
    // at 1000 and its high reached 1500, taking the previous high on the way.
    assert_eq!(body["bias"]["previous_mid"], 1050.0);
    assert_eq!(body["bias"]["open_above_mid"], false);
    assert_eq!(body["bias"]["touched_pdh"], true);
    assert_eq!(body["bias"]["first_taken"], "PDH");

    // Closing back under the midpoint after taking the previous high is the
    // reversal signature, and it outranks where the session opened.
    assert_eq!(body["bias"]["read"], "reversal_depuis_le_haut");

    // The midpoint is also offered as a level to mark on the chart.
    let distances = body["distances"].as_array().unwrap();
    assert!(distances.iter().any(|d| d["key"] == "previous_mid"));
}

#[tokio::test]
async fn brief_reports_which_weekday_made_the_week_extremes() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("NAS100")).await;

    // 2026-07-30 is a Thursday; the week opened on Monday 2026-07-27.
    let body = get_brief(&app, &token, "NAS100", "2026-07-30").await;

    assert_eq!(body["weekly"]["today_weekday"], "jeudi");
    assert_eq!(body["weekly"]["week_start"], "2026-07-27");
    // Thursday is the 500-point session, so it holds the weekly high.
    assert_eq!(body["weekly"]["high_weekday"], "jeudi");
    assert_eq!(body["weekly"]["high"], 1500.0);
}

#[tokio::test]
async fn brief_stays_silent_rather_than_averaging_three_sessions() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(
        &app,
        vec![session_batch("XAUUSD", "2026-07-29", 1100.0, 1000.0)],
    )
    .await;

    let body = get_brief(&app, &token, "XAUUSD", "2026-07-30").await;

    assert!(body["stats"]["adr"].is_null(), "no ADR on a single session");
    assert_eq!(body["bias"]["read"], "indetermine");
}

#[tokio::test]
async fn brief_rejects_a_malformed_date() {
    let app = test_app().await;
    let token = login(&app).await;
    let req = Request::builder()
        .uri("/brief?symbol=GER40&date=30-07-2026")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

// --- Brief archive ----------------------------------------------------------

async fn snapshot(app: &axum::Router, body: Value) -> axum::response::Response {
    let req = Request::builder()
        .method("POST")
        .uri("/brief/snapshot")
        .header("content-type", "application/json")
        .header("x-ingest-token", TEST_INGEST_TOKEN)
        .body(Body::from(body.to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn authed_get(app: &axum::Router, token: &str, uri: &str) -> Value {
    let req = Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "GET {uri}");
    body_json(res).await
}

#[tokio::test]
async fn snapshot_requires_the_machine_token() {
    let app = test_app().await;
    let token = login(&app).await;

    // No credential at all.
    let req = Request::builder()
        .method("POST")
        .uri("/brief/snapshot")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "session": "pre_london" }).to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    // A user JWT is not a machine credential.
    let req = Request::builder()
        .method("POST")
        .uri("/brief/snapshot")
        .header("content-type", "application/json")
        .header("x-ingest-token", token)
        .body(Body::from(json!({ "session": "pre_london" }).to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn snapshot_rejects_an_unknown_session() {
    let app = test_app().await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    let res = snapshot(&app, json!({ "session": "midnight", "date": "2026-07-30" })).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// The whole point of the archive: what was captured before the session must
/// never be replaced by a later view of the same day.
#[tokio::test]
async fn the_first_capture_of_a_session_wins() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    let first = snapshot(
        &app,
        json!({ "session": "pre_london", "date": "2026-07-30", "symbols": ["GER40"] }),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let body = body_json(first).await;
    assert_eq!(body["captured"], json!(["GER40"]));
    assert!(body["already_present"].as_array().unwrap().is_empty());

    let second = snapshot(
        &app,
        json!({ "session": "pre_london", "date": "2026-07-30", "symbols": ["GER40"] }),
    )
    .await;
    let body = body_json(second).await;
    assert!(body["captured"].as_array().unwrap().is_empty());
    assert_eq!(body["already_present"], json!(["GER40"]));

    // Still exactly one row.
    let history = authed_get(&app, &token, "/brief/history?symbol=GER40").await;
    assert_eq!(history.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_two_daily_sessions_are_archived_separately() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    for session in ["pre_london", "pre_ny"] {
        let res = snapshot(
            &app,
            json!({ "session": session, "date": "2026-07-30", "symbols": ["GER40"] }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    let history = authed_get(&app, &token, "/brief/history?symbol=GER40").await;
    let rows = history.as_array().unwrap();
    assert_eq!(rows.len(), 2);

    let sessions: Vec<&str> = rows.iter().map(|r| r["session"].as_str().unwrap()).collect();
    assert!(sessions.contains(&"pre_london"));
    assert!(sessions.contains(&"pre_ny"));
}

#[tokio::test]
async fn an_archived_brief_is_read_back_verbatim() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    snapshot(
        &app,
        json!({ "session": "pre_london", "date": "2026-07-30", "symbols": ["GER40"] }),
    )
    .await;

    let archived = authed_get(
        &app,
        &token,
        "/brief/archived?symbol=GER40&date=2026-07-30&session=pre_london",
    )
    .await;

    assert_eq!(archived["archived"], true);
    assert_eq!(archived["session"], "pre_london");
    // The frozen payload carries the whole brief, levels included.
    assert_eq!(archived["brief"]["stats"]["adr"], 100.0);
    assert_eq!(archived["brief"]["bias"]["previous_mid"], 1050.0);
    assert_eq!(archived["brief"]["levels"]["day"]["high"], 1500.0);
}

#[tokio::test]
async fn reading_back_a_day_that_was_never_captured_is_a_404() {
    let app = test_app().await;
    let token = login(&app).await;

    let req = Request::builder()
        .uri("/brief/archived?symbol=GER40&date=2026-07-30&session=pre_london")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_review_puts_the_brief_next_to_the_trades_of_that_day() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_account(&app, &token).await;
    seed_candles(&app, quiet_then_violent("GER40")).await;

    snapshot(
        &app,
        json!({ "session": "pre_london", "date": "2026-07-30", "symbols": ["GER40"] }),
    )
    .await;

    // Two trades that day, one on the reviewed instrument.
    for (symbol, pnl, ticket) in [("GER40", 120.0, "R-1"), ("US30", -40.0, "R-2")] {
        let req = Request::builder()
            .method("POST")
            .uri("/trades")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({
                    "symbol": symbol,
                    "pnl": pnl,
                    "open_time": "2026-07-30T09:15:00",
                    "mt5_ticket": ticket
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::CREATED
        );
    }

    let review = authed_get(
        &app,
        &token,
        "/brief/review?symbol=GER40&from=2026-07-01&to=2026-07-31",
    )
    .await;

    let day = review
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == "2026-07-30")
        .expect("the captured day is in the review");

    assert_eq!(day["snapshots"].as_array().unwrap().len(), 1);
    // Every trade of the day counts towards "am I trading more?"...
    assert_eq!(day["trades_all"]["count"], 2);
    assert_eq!(day["trades_all"]["pnl"], 80.0);
    // ...but only the reviewed instrument counts towards its own tally.
    assert_eq!(day["trades_symbol"]["count"], 1);
    assert_eq!(day["trades_symbol"]["pnl"], 120.0);
}

/// Days traded without a brief are the baseline. Hiding them would flatter the
/// comparison by only ever showing days the brief covered.
#[tokio::test]
async fn the_review_keeps_days_that_have_trades_but_no_brief() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_account(&app, &token).await;

    let req = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "symbol": "GER40",
                "pnl": 250.0,
                "open_time": "2026-07-15T10:00:00",
                "mt5_ticket": "B-1"
            })
            .to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::CREATED
    );

    let review = authed_get(&app, &token, "/brief/review?from=2026-07-01&to=2026-07-31").await;
    let day = review
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == "2026-07-15")
        .expect("a traded day with no snapshot still appears");

    assert!(day["snapshots"].as_array().unwrap().is_empty());
    assert_eq!(day["trades_all"]["count"], 1);
}

// --- Trade ingestion from the MT5 side --------------------------------------

/// Create an account bound to an MT5 login, and return its id.
async fn seed_bound_account(app: &axum::Router, token: &str, login: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "name": "Compte démo",
                "broker": "XM",
                "mt5_login": login,
                "mt5_server": "BrokerDemo-MT5 1"
            })
            .to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body = body_json(res).await;
    assert_eq!(body["mt5_login"], login);
    body["id"].as_str().unwrap().to_string()
}

fn ingest_trades_request(token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/ingest/trades")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("x-ingest-token", t);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

/// One leg of a plan, still open.
fn open_leg(login: &str, ticket: &str) -> Value {
    json!({
        "login": login,
        "trades": [{
            "ticket": ticket,
            "symbol": "GER40Cash#",
            "direction": "buy",
            "open_time": "2026-11-06 08:15:00",
            "open_price": 26300.0,
            "stop_loss": 26250.0,
            "take_profit": 26400.0,
            "lot_size": 6.7,
            "pnl": -12.5,
            "plan_id": "plan-abc"
        }]
    })
}

#[tokio::test]
async fn ingesting_trades_needs_the_machine_token_not_the_jwt() {
    let app = test_app().await;
    let jwt = login(&app).await;

    let res = app
        .clone()
        .oneshot(ingest_trades_request(None, open_leg("100000001", "1")))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let res = app
        .clone()
        .oneshot(ingest_trades_request(
            Some(&jwt),
            open_leg("100000001", "1"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ingesting_trades_refuses_a_login_no_account_claims() {
    let app = test_app().await;
    let token = login(&app).await;
    // An account exists, but it declares no MT5 login, so it is unreachable
    // from the machine credential.
    seed_account(&app, &token).await;

    let res = app
        .clone()
        .oneshot(ingest_trades_request(
            Some(TEST_INGEST_TOKEN),
            open_leg("100000001", "21455031"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn replaying_a_trade_updates_it_instead_of_duplicating_it() {
    let app = test_app().await;
    let token = login(&app).await;
    let account_id = seed_bound_account(&app, &token, "100000001").await;

    let res = app
        .clone()
        .oneshot(ingest_trades_request(
            Some(TEST_INGEST_TOKEN),
            open_leg("100000001", "21455031"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["account_id"], account_id);
    assert_eq!(body["created"], 1);
    assert_eq!(body["updated"], 0);

    // The position closes: same ticket, now with an exit.
    let closed = json!({
        "login": "100000001",
        "trades": [{
            "ticket": "21455031",
            "symbol": "GER40Cash#",
            "direction": "buy",
            "open_time": "2026-11-06 08:15:00",
            "close_time": "2026-11-06 10:42:00",
            "open_price": 26300.0,
            "close_price": 26400.0,
            "lot_size": 6.7,
            "pnl": 670.0,
            "plan_id": "plan-abc"
        }]
    });
    let res = app
        .clone()
        .oneshot(ingest_trades_request(Some(TEST_INGEST_TOKEN), closed))
        .await
        .unwrap();
    let body = body_json(res).await;
    assert_eq!(body["created"], 0);
    assert_eq!(body["updated"], 1);

    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let trades = body_json(app.clone().oneshot(req).await.unwrap()).await;
    let trades = trades.as_array().unwrap();
    assert_eq!(trades.len(), 1, "one ticket must stay one row");
    assert_eq!(trades[0]["mt5_ticket"], "100000001:21455031");
    assert_eq!(trades[0]["plan_id"], "plan-abc");
    assert_eq!(trades[0]["pnl"], 670.0);
    assert_eq!(trades[0]["close_price"], 26400.0);
    assert_eq!(trades[0]["direction"], "LONG");
}

#[tokio::test]
async fn replaying_a_trade_never_overwrites_the_review() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_bound_account(&app, &token, "100000001").await;

    app.clone()
        .oneshot(ingest_trades_request(
            Some(TEST_INGEST_TOKEN),
            open_leg("100000001", "21455031"),
        ))
        .await
        .unwrap();

    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let trades = body_json(app.clone().oneshot(req).await.unwrap()).await;
    let trade_id = trades[0]["id"].as_str().unwrap().to_string();

    // The trader reviews the trade in the app.
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/trades/{trade_id}"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "notes": "sortie trop tôt sur le retest",
                "setup_tag": "ORB Londres",
                "followed_plan": true
            })
            .to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );

    // The terminal pushes the same position again.
    app.clone()
        .oneshot(ingest_trades_request(
            Some(TEST_INGEST_TOKEN),
            open_leg("100000001", "21455031"),
        ))
        .await
        .unwrap();

    let req = Request::builder()
        .uri(format!("/trades/{trade_id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let trade = body_json(app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(trade["notes"], "sortie trop tôt sur le retest");
    assert_eq!(trade["setup_tag"], "ORB Londres");
    assert_eq!(trade["followed_plan"], true);
}

#[tokio::test]
async fn the_same_broker_ticket_on_two_terminals_makes_two_trades() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_bound_account(&app, &token, "100000001").await;

    // A second bound account, on another terminal.
    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "Autre", "mt5_login": "100000002" }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::CREATED
    );

    for login_number in ["100000001", "100000002"] {
        let res = app
            .clone()
            .oneshot(ingest_trades_request(
                Some(TEST_INGEST_TOKEN),
                open_leg(login_number, "5001"),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["created"], 1);
    }

    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let trades = body_json(app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(trades.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn two_accounts_cannot_claim_the_same_mt5_login() {
    let app = test_app().await;
    let token = login(&app).await;
    seed_bound_account(&app, &token, "100000001").await;

    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "Doublon", "mt5_login": "100000001" }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn an_mt5_login_must_be_a_number() {
    let app = test_app().await;
    let token = login(&app).await;

    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "Bancal", "mt5_login": "100000001:9" }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn unbinding_an_account_frees_the_login_for_another() {
    let app = test_app().await;
    let token = login(&app).await;
    let first = seed_bound_account(&app, &token, "100000001").await;

    // An empty string clears the binding; omitting the field would keep it.
    let req = Request::builder()
        .method("PUT")
        .uri(format!("/accounts/{first}"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(json!({ "mt5_login": "" }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body_json(res).await["mt5_login"].is_null());

    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "Repreneur", "mt5_login": "100000001" }).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn closing_a_position_keeps_the_stop_it_was_opened_with() {
    // MT5 reports a closed position without the stop it carried at entry.
    // Losing it would make the R multiple of every automated trade
    // uncomputable after the fact.
    let app = test_app().await;
    let token = login(&app).await;
    seed_bound_account(&app, &token, "100000001").await;

    app.clone()
        .oneshot(ingest_trades_request(
            Some(TEST_INGEST_TOKEN),
            open_leg("100000001", "21455031"),
        ))
        .await
        .unwrap();

    let closed = json!({
        "login": "100000001",
        "trades": [{
            "ticket": "21455031",
            "symbol": "GER40Cash#",
            "close_time": "2026-11-06 10:42:00",
            "close_price": 26400.0,
            "pnl": 670.0
        }]
    });
    assert_eq!(
        app.clone()
            .oneshot(ingest_trades_request(Some(TEST_INGEST_TOKEN), closed))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let trades = body_json(app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(trades[0]["stop_loss"], 26250.0);
    assert_eq!(trades[0]["open_price"], 26300.0);
    assert_eq!(trades[0]["plan_id"], "plan-abc");
    assert_eq!(trades[0]["close_price"], 26400.0);
}

// --- Prop firm status --------------------------------------------------------

/// Create an account with a starting balance, and return its id.
async fn seed_funded_account(app: &axum::Router, token: &str, balance: f64) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/accounts")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({ "name": "Challenge", "balance": balance, "currency": "USD" }).to_string(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    body_json(res).await["id"].as_str().unwrap().to_string()
}

async fn add_closed_trade(
    app: &axum::Router,
    token: &str,
    account_id: &str,
    close_time: &str,
    pnl: f64,
) {
    let req = Request::builder()
        .method("POST")
        .uri("/trades")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "account_id": account_id,
                "symbol": "GER40",
                "direction": "LONG",
                "open_time": close_time,
                "close_time": close_time,
                "pnl": pnl
            })
            .to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn prop_status_without_a_starting_balance_explains_itself() {
    let app = test_app().await;
    let token = login(&app).await;
    // seed_account creates one with no balance at all.
    let account_id = seed_account(&app, &token).await;

    let req = Request::builder()
        .uri(format!("/accounts/{account_id}/prop-status"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let error = body_json(res).await["error"].as_str().unwrap().to_string();
    assert!(error.contains("solde de départ"), "got: {error}");
}

#[tokio::test]
async fn prop_status_measures_drawdown_from_the_peak() {
    let app = test_app().await;
    let token = login(&app).await;
    let account_id = seed_funded_account(&app, &token, 100_000.0).await;

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/accounts/{account_id}/rules"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "daily_drawdown_max": 0.05,
                "global_drawdown_max": 0.10,
                "profit_target": 0.10,
                "consistency_rule_pct": 0.20
            })
            .to_string(),
        ))
        .unwrap();
    assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);

    add_closed_trade(&app, &token, &account_id, "2026-11-02T10:00:00", 8_000.0).await;
    add_closed_trade(&app, &token, &account_id, "2026-11-03T10:00:00", -5_000.0).await;

    let req = Request::builder()
        .uri(format!("/accounts/{account_id}/prop-status"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let status = body_json(res).await;

    assert_eq!(status["peak_equity"], 108_000.0);
    assert_eq!(status["current_equity"], 103_000.0);
    assert_eq!(status["current_drawdown"], 5_000.0);
    assert_eq!(status["drawdown_used"], 0.5);
    assert_eq!(status["daily_limit"], 5_000.0);
    // Neither trade closed today, so today's allowance is untouched.
    assert_eq!(status["today_pnl"], 0.0);
    assert_eq!(status["best_day"], "2026-11-02");
}

// --- Grouped statistics ------------------------------------------------------

#[tokio::test]
async fn stats_group_by_setup_splits_the_population() {
    let app = test_app().await;
    let token = login(&app).await;
    let account_id = seed_funded_account(&app, &token, 100_000.0).await;

    for (setup, pnl) in [("ORB", 300.0), ("ORB", -100.0), ("Sweep", 500.0)] {
        let req = Request::builder()
            .method("POST")
            .uri("/trades")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(
                json!({
                    "account_id": account_id,
                    "symbol": "GER40",
                    "open_time": "2026-11-02T10:00:00",
                    "close_time": "2026-11-02T11:00:00",
                    "setup_tag": setup,
                    "pnl": pnl
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::CREATED
        );
    }

    let req = Request::builder()
        .uri("/trades/stats?group_by=setup")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let stats = body_json(app.clone().oneshot(req).await.unwrap()).await;

    assert_eq!(stats["group_by"], "setup");
    let groups = stats["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    // Ordered by P&L: Sweep (500) before ORB (200).
    assert_eq!(groups[0]["key"], "Sweep");
    assert_eq!(groups[0]["trades"], 1);
    assert_eq!(groups[0]["profit_factor"], Value::Null, "pas de perte");
    assert_eq!(groups[1]["key"], "ORB");
    assert_eq!(groups[1]["trades"], 2);
    assert_eq!(groups[1]["wins"], 1);
    assert_eq!(groups[1]["losses"], 1);
    assert_eq!(groups[1]["pnl"], 200.0);
    assert_eq!(groups[1]["win_rate"], 0.5);
    assert_eq!(groups[1]["profit_factor"], 3.0);

    // The old field keeps its old meaning.
    assert_eq!(stats["by_symbol"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn stats_without_group_by_are_unchanged() {
    let app = test_app().await;
    let token = login(&app).await;

    let req = Request::builder()
        .uri("/trades/stats")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let stats = body_json(app.clone().oneshot(req).await.unwrap()).await;
    assert!(stats["groups"].as_array().unwrap().is_empty());
    assert_eq!(stats["group_by"], Value::Null);
}

#[tokio::test]
async fn an_unknown_group_by_lists_the_valid_ones() {
    let app = test_app().await;
    let token = login(&app).await;

    let req = Request::builder()
        .uri("/trades/stats?group_by=semaine")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let error = body_json(res).await["error"].as_str().unwrap().to_string();
    assert!(error.contains("week"), "got: {error}");
}

// --- MCP ---------------------------------------------------------------------

fn mcp_request(token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

fn rpc(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

async fn mcp_call(app: &axum::Router, tool: &str, arguments: Value) -> Value {
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            rpc(1, "tools/call", json!({ "name": tool, "arguments": arguments })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

#[tokio::test]
async fn mcp_refuses_a_missing_or_wrong_token() {
    let app = test_app().await;

    for token in [None, Some("pas-le-bon")] {
        let res = app
            .clone()
            .oneshot(mcp_request(token, rpc(1, "tools/list", json!({}))))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn mcp_does_not_accept_the_user_jwt() {
    // The three credentials are separate on purpose: a JWT must not open the
    // MCP surface, and the MCP token must not open the user API.
    let app = test_app().await;
    let jwt = login(&app).await;
    let res = app
        .clone()
        .oneshot(mcp_request(Some(&jwt), rpc(1, "tools/list", json!({}))))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // And the reverse.
    let req = Request::builder()
        .uri("/trades")
        .header("authorization", format!("Bearer {TEST_MCP_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn mcp_initialize_announces_the_server() {
    let app = test_app().await;
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            rpc(
                1,
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = body_json(res).await;
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], 1);
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(body["result"]["serverInfo"]["name"], "trading-journal");
    assert!(body["result"]["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn mcp_notifications_get_no_body() {
    // A notification carries no id; answering one would leave a client that
    // correlates on id waiting for a reply to a question it never asked.
    let app = test_app().await;
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn mcp_lists_its_tools() {
    let app = test_app().await;
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            rpc(2, "tools/list", json!({})),
        ))
        .await
        .unwrap();
    let body = body_json(res).await;
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "list_accounts",
        "get_trades",
        "get_stats",
        "get_prop_status",
        "get_brief",
    ] {
        assert!(names.contains(&expected), "{expected} manquant dans {names:?}");
    }
}

#[tokio::test]
async fn mcp_unknown_method_is_a_protocol_error() {
    let app = test_app().await;
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            rpc(3, "tools/invoke", json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "JSON-RPC répond en 200");
    let body = body_json(res).await;
    assert_eq!(body["error"]["code"], -32601);
}

#[tokio::test]
async fn mcp_unknown_tool_is_reported_to_the_model_not_to_the_transport() {
    // A failing tool comes back as a successful result carrying isError, so
    // the model can read the message and correct itself.
    let app = test_app().await;
    let body = mcp_call(&app, "supprime_tout", json!({})).await;
    assert!(body["error"].is_null());
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("supprime_tout"));
}

#[tokio::test]
async fn mcp_reads_accounts_and_grouped_stats() {
    let app = test_app().await;
    let token = login(&app).await;
    let account_id = seed_funded_account(&app, &token, 100_000.0).await;
    add_closed_trade(&app, &token, &account_id, "2026-11-02T10:00:00", 400.0).await;
    add_closed_trade(&app, &token, &account_id, "2026-11-03T10:00:00", -150.0).await;

    let body = mcp_call(&app, "list_accounts", json!({})).await;
    let result = &body["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["count"], 1);
    // The text block mirrors the structured one, for clients that read only text.
    assert!(result["content"][0]["text"].as_str().unwrap().contains("Challenge"));

    let body = mcp_call(&app, "get_stats", json!({ "group_by": "day" })).await;
    let stats = &body["result"]["structuredContent"];
    assert_eq!(stats["total_trades"], 2);
    assert_eq!(stats["group_by"], "day");
    let groups = stats["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    // Chronological, because a month-by-month table sorted by profit is unreadable.
    assert_eq!(groups[0]["key"], "2026-11-02");
    assert_eq!(groups[1]["key"], "2026-11-03");
}

#[tokio::test]
async fn mcp_tool_arguments_are_validated_like_the_rest() {
    let app = test_app().await;

    // Required argument missing.
    let body = mcp_call(&app, "get_prop_status", json!({})).await;
    assert_eq!(body["result"]["isError"], true);

    // Unknown grouping, refused by the same whitelist the REST route uses.
    let body = mcp_call(&app, "get_stats", json!({ "group_by": "lune" })).await;
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("week"), "l'erreur doit lister les valeurs valides");
}

#[tokio::test]
async fn mcp_has_no_tool_that_writes() {
    // The guard that matters: this surface is read-only, and a write tool
    // added by accident should fail here rather than in production.
    let app = test_app().await;
    let res = app
        .clone()
        .oneshot(mcp_request(
            Some(TEST_MCP_TOKEN),
            rpc(9, "tools/list", json!({})),
        ))
        .await
        .unwrap();
    let body = body_json(res).await;
    let forbidden = [
        "create", "update", "delete", "set_", "put_", "post_", "write", "import", "execute",
        "place", "close", "modify", "supprim", "cree", "créer", "modifi",
    ];
    for tool in body["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap().to_lowercase();
        for verb in forbidden {
            assert!(
                !name.contains(verb),
                "'{name}' ressemble à une écriture : /mcp doit rester en lecture seule"
            );
        }
    }
}

#[tokio::test]
async fn mcp_does_not_pretend_to_stream() {
    let app = test_app().await;
    let req = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header("authorization", format!("Bearer {TEST_MCP_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    // 405 and not 404: the endpoint exists, it just takes no GET.
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}
