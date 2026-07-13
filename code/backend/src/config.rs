//! Runtime configuration, loaded from environment variables.

use anyhow::Context;

#[derive(Clone, Debug)]
pub struct Config {
    /// SQLx connection string, e.g. `sqlite://data/journal.db`.
    pub database_url: String,
    /// Secret used to sign/verify JWTs (HS256).
    pub jwt_secret: String,
    /// Argon2 PHC hash of the admin password. Generate with `app hash <password>`.
    pub admin_password_hash: String,
    /// Socket address the server binds to, e.g. `0.0.0.0:8080`.
    pub bind_addr: String,
    /// JWT lifetime in hours.
    pub jwt_ttl_hours: i64,
    /// Comma-separated list of allowed CORS origins, or `*`.
    pub cors_allowed_origins: String,
}

impl Config {
    /// Build a `Config` from environment variables, applying sensible defaults
    /// for everything except the security-critical secrets.
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env_or("DATABASE_URL", "sqlite://journal.db"),
            jwt_secret: require_env("JWT_SECRET")?,
            admin_password_hash: require_env("ADMIN_PASSWORD_HASH")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            jwt_ttl_hours: env_or("JWT_TTL_HOURS", "168") // 7 days
                .parse()
                .context("JWT_TTL_HOURS must be an integer")?,
            cors_allowed_origins: env_or("CORS_ALLOWED_ORIGINS", "*"),
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn require_env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("environment variable {key} is required"))
}
