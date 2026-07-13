//! Trading Journal API library crate.
//!
//! Exposing the modules from a library (rather than only `main.rs`) lets the
//! integration tests in `tests/` drive the router in-process.

pub mod auth;
pub mod config;
pub mod csv_import;
pub mod db;
pub mod error;
pub mod models;
pub mod routes;
pub mod state;
