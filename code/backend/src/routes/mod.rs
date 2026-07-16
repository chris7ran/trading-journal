//! HTTP routing: assembles public and protected routes into a single Router.

pub mod accounts;
pub mod health;
pub mod import;
pub mod market;
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
        .route("/trades", get(trades::list_trades).post(trades::create_trade))
        // Static segment registered before the `:id` param route.
        .route("/trades/stats", get(stats::trade_stats))
        .route("/trades/import/csv", post(import::import_csv))
        .route("/trades/:id", get(trades::get_trade).put(trades::update_trade))
        .route("/accounts", get(accounts::list_accounts).post(accounts::create_account))
        .route("/accounts/:id", put(accounts::update_account))
        .route(
            "/accounts/:id/rules",
            get(accounts::get_rules).put(accounts::upsert_rules),
        )
        .route("/setups", get(setups::list_setups).post(setups::create_setup))
        .route("/setups/:id", put(setups::update_setup))
        .route("/macro/calendar", get(market::calendar))
        .route("/macro/news", get(market::news))
        .route("/macro/economy", get(market::economy))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::require_auth,
        ));

    // Public routes (no auth).
    let public = Router::new()
        .route("/health", get(health::health))
        .route(
            "/auth/login",
            post(auth_handlers::login).layer(GovernorLayer {
                config: login_rate_limit,
            }),
        );

    public
        .merge(protected)
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
