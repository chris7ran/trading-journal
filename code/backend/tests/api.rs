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
