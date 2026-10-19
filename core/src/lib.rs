//! Research Dome core.
//!
//! The original application was a Next.js UI talking to a FastAPI service backed
//! by PostgreSQL. This crate is the Rust replacement for the service and the
//! database: the same API surface, the same behaviour, SQLite instead of
//! PostgreSQL, and a single self-contained binary instead of a Python runtime.

pub mod api;
pub mod config;
pub mod db;
pub mod error;
pub mod latex;
pub mod models;
pub mod pdf_export;
pub mod repo;
pub mod server;
pub mod storage;
pub mod util;

pub use config::Config;
pub use error::{AppError, AppResult};
pub use server::{generate_token, init_logging, start, ServerHandle};
