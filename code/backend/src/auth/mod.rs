//! Authentication: password hashing, JWT issuing/verification, and the
//! middleware that guards protected routes.

pub mod handlers;
pub mod jwt;
pub mod middleware;
pub mod password;
