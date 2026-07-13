//! Argon2 password hashing helpers.
//!
//! The admin password is never stored in plaintext. We keep only its Argon2
//! PHC hash in the `ADMIN_PASSWORD_HASH` environment variable. Generate it with
//! the bundled CLI subcommand: `trading-journal-api hash <password>`.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::OsRng;

/// Hash a plaintext password into an Argon2 PHC string.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("failed to hash password: {e}"))?
        .to_string();
    Ok(hash)
}

/// Verify a plaintext password against a stored Argon2 PHC hash.
/// Returns `false` for any mismatch or malformed hash rather than erroring.
pub fn verify_password(password: &str, phc_hash: &str) -> bool {
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}
