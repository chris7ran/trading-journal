//! HTTP routing: assembles public and protected routes into a single Router.

pub mod accounts;
pub mod archive;
pub mod candles;
pub mod health;
pub mod import;
pub mod ingest;
pub mod ingest_trades;
pub mod levels;
pub mod market;
pub mod mcp;
pub mod setups;
pub mod stats;
pub mod trades;

use std::sync::Arc;

use axum::routing::{get, post, put};
use axum::Router;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::GovernorLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::{handlers as auth_handlers, middleware as auth_middleware};
use crate::state::AppState;

/// Build the full application router.
pub fn build_router(state: AppState) -> Router {
    let cors = build_cors(&state.config.cors_allowed_origins);

    // Rate limit for `/auth/login`: burst of 5 attempts, then one every 12s
    // (~5 per minute steady state), keyed by peer IP. Brute-force guard — the
    // tailnet's WireGuard/ACL layer authenticates devices, not login attempts.
    let login_rate_limit = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(12)
            .burst_size(5)
            .finish()
            .expect("valid governor rate-limit config"),
    );

    // Routes that require a valid JWT.
    let protected = Router::new()
        .route("/auth/verify-token", post(auth_handlers::verify_token))
        .route(
            "/trades",
            get(trades::list_trades).post(trades::create_trade),
        )
        // Static segment registered before the `:id` param route.
        .route("/trades/stats", get(stats::trade_stats))
        .route("/trades/import/csv", post(import::import_csv))
        .route(
            "/trades/:id",
            get(trades::get_trade)
                .put(trades::update_trade)
                .delete(trades::delete_trade),
        )
        .route(
            "/accounts",
            get(accounts::list_accounts).post(accounts::create_account),
        )
        .route(
            "/accounts/:id",
            put(accounts::update_account).delete(accounts::delete_account),
        )
        .route(
            "/accounts/:id/rules",
            get(accounts::get_rules).put(accounts::upsert_rules),
        )
        .route("/accounts/:id/prop-status", get(accounts::prop_status))
        .route(
            "/setups",
            get(setups::list_setups).post(setups::create_setup),
        )
        .route("/setups/:id", put(setups::update_setup))
        .route("/macro/calendar", get(market::calendar))
        .route("/macro/news", get(market::news))
        .route("/macro/economy", get(market::economy))
        .route("/macro/cot", get(market::cot))
        // Chart bars for the levels screen, folded from the same M5 feed the
        // levels themselves are computed from.
        .route("/candles", get(candles::get_candles))
        // Static segment before any future `/levels/:something` param route.
        .route("/levels/symbols", get(levels::list_symbols))
        .route("/levels", get(levels::get_levels))
        .route("/brief", get(levels::get_brief))
        // Static segments before `/brief` itself would be ambiguous, so the
        // archive lives on its own paths.
        .route("/brief/history", get(archive::history))
        .route("/brief/archived", get(archive::get_snapshot))
        .route("/brief/review", get(archive::review))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::require_auth,
        ));

    // Machine-to-machine ingestion from the MT5 Expert Advisor. Not under the
    // JWT: it authenticates with its own `X-Ingest-Token`, which grants
    // write-only access to candles and nothing else.
    //
    // The token is verified in a `route_layer` rather than inside the handlers,
    // so an unauthenticated request is rejected before its body is buffered or
    // deserialised.
    //
    // No rate limiter here, unlike `/auth/login`: the EA's initial backfill is a
    // legitimate burst of ~130 requests, and the tailnet's WireGuard/ACL layer
    // already gates who can reach the port at all.
    let machine = Router::new()
        .route("/ingest/candles", post(ingest::ingest_candles))
        .route("/ingest/status", get(ingest::ingest_status))
        // Trades executed on the MT5 side. Resolves its target account from
        // the login it declares, so this credential can only write into
        // accounts that are actually bound to a terminal.
        .route("/ingest/trades", post(ingest_trades::ingest_trades))
        // Freezing the brief is a cron's job, not a person's — same machine
        // credential as the candle feed.
        .route("/brief/snapshot", post(archive::create_snapshot))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            ingest::require_ingest_token,
        ));

    // The journal as an MCP server, for an assistant reading over the
    // tailnet. A third credential rather than the JWT: that one expires, and
    // a connector left in place for weeks would break every time it did.
    //
    // Read-only, and on its own router so that stays visible — the day a
    // write tool is considered, it has to be added here, in the open.
    let mcp = Router::new()
        .route("/mcp", post(mcp::handle).get(mcp::stream_not_supported))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            mcp::require_mcp_token,
        ));

    // Public routes (no auth).
    let public = Router::new().route("/health", get(health::health)).route(
        "/auth/login",
        post(auth_handlers::login).layer(GovernorLayer {
            config: login_rate_limit,
        }),
    );

    public
        .merge(protected)
        .merge(machine)
        .merge(mcp)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// Build a CORS layer from the configured origins (`*` allows any).
fn build_cors(origins: &str) -> CorsLayer {
    use axum::http::{HeaderValue, Method};
    let methods = vec![Method::GET, Method::POST, Method::PUT, Method::DELETE];

    if origins.trim() == "*" {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(methods)
            .allow_headers(Any)
    } else {
        let list: Vec<HeaderValue> = origins
            .split(',')
            .filter_map(|o| o.trim().parse::<HeaderValue>().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(list)
            .allow_methods(methods)
            .allow_headers(Any)
    }
}
