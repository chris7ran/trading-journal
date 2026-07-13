//! Bearer-token auth middleware guarding protected routes.

use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::auth::jwt::{verify_token, Claims};
use crate::error::AppError;
use crate::state::AppState;

/// Reject any request lacking a valid `Authorization: Bearer <jwt>` header.
/// On success, the decoded `Claims` are inserted into request extensions so
/// downstream handlers can read the authenticated subject if needed.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;

    let claims: Claims = verify_token(&state.config.jwt_secret, token.trim())?;
    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}
