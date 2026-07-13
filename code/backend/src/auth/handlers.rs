//! Auth HTTP handlers: login and token verification.

use axum::extract::State;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::jwt::{issue_token, Claims};
use crate::auth::password::verify_password;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub token_type: &'static str,
    pub expires_in_hours: i64,
}

/// `POST /auth/login` — exchange the admin password for a JWT.
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<TokenResponse>> {
    if !verify_password(&body.password, &state.config.admin_password_hash) {
        return Err(AppError::Unauthorized);
    }

    let token = issue_token(&state.config.jwt_secret, "admin", state.config.jwt_ttl_hours)
        .map_err(AppError::Other)?;

    Ok(Json(TokenResponse {
        token,
        token_type: "Bearer",
        expires_in_hours: state.config.jwt_ttl_hours,
    }))
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
    pub subject: String,
    pub expires_at: i64,
}

/// `POST /auth/verify-token` — protected; confirms the bearer token is valid.
/// The middleware already validated the token and inserted the claims.
pub async fn verify_token(Extension(claims): Extension<Claims>) -> Json<VerifyResponse> {
    Json(VerifyResponse {
        valid: true,
        subject: claims.sub,
        expires_at: claims.exp,
    })
}
