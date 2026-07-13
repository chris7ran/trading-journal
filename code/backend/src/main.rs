//! Trading Journal API — binary entry point.
//!
//! Usage:
//!   trading-journal-api            Run the HTTP server.
//!   trading-journal-api hash PWD   Print an Argon2 hash for PWD and exit
//!                                  (paste it into ADMIN_PASSWORD_HASH).

use anyhow::Context;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use trading_journal_api::config::Config;
use trading_journal_api::state::AppState;
use trading_journal_api::{auth, db, routes};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present (no-op in production where env is injected directly).
    let _ = dotenvy::dotenv();

    // `hash` subcommand: generate an Argon2 hash and exit. Runs before logging
    // setup so the output is clean and copy-pasteable.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hash") {
        return run_hash_command(args.get(2));
    }

    init_tracing();

    let config = Config::from_env().context("failed to load configuration")?;
    let pool = db::init_pool(&config.database_url).await?;
    let bind_addr = config.bind_addr.clone();

    let state = AppState::new(pool, config);
    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;

    tracing::info!("trading-journal-api listening on {bind_addr}");
    axum::serve(listener, app).await.context("server error")?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn run_hash_command(password: Option<&String>) -> anyhow::Result<()> {
    let password = password.context("usage: trading-journal-api hash <password>")?;
    let hash = auth::password::hash_password(password)?;
    println!("{hash}");
    eprintln!("\n# Put this in your .env / systemd EnvironmentFile:");
    eprintln!("ADMIN_PASSWORD_HASH='{hash}'");
    Ok(())
}
