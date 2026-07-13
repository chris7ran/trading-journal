//! JWT issuing and verification (HS256).

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// JWT claims. `sub` identifies the user; for this single-user app it is "admin".
/// `Clone` is required because the auth middleware stores `Claims` in request
/// extensions, and the `Extension<Claims>` extractor clones it out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    /// Expiry (unix timestamp, seconds).
    pub exp: i64,
    /// Issued-at (unix timestamp, seconds).
    pub iat: i64,
}

/// Create a signed JWT for the given subject.
pub fn issue_token(secret: &str, subject: &str, ttl_hours: i64) -> anyhow::Result<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: subject.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::hours(ttl_hours)).timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// Verify a JWT and return its claims, or `AppError::Unauthorized` if invalid.
pub fn verify_token(secret: &str, token: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized)
}
