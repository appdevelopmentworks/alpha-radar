//! Application-wide error type.
//!
//! Scans collect per-row failures (see `RowError`, added in P4) and never abort
//! the whole run; `AppError` is the top-level error surfaced across the Tauri
//! command boundary (docs/01-architecture.md "横断的関心事").

use thiserror::Error;

/// Top-level error for the Rust core.
#[derive(Debug, Error)]
pub enum AppError {
    /// Caller supplied an invalid argument (bad CSV field, unknown symbol, …).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Not enough bars to compute the requested indicator/window.
    #[error("insufficient data: need at least {needed} bars, got {got}")]
    InsufficientData { needed: usize, got: usize },

    /// Filesystem / cache I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// (De)serialization failure (sidecar JSON, config, …).
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Python sidecar process / protocol failure.
    #[error("sidecar error: {0}")]
    Sidecar(String),

    /// SQLite cache failure.
    #[error("cache error: {0}")]
    Cache(#[from] rusqlite::Error),

    /// CSV parse failure (file-level; per-row failures become `RowError`).
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
}

/// Convenience alias for fallible core operations.
pub type AppResult<T> = Result<T, AppError>;

// Serialize as the display string so Tauri commands can return `AppError`
// directly across the IPC boundary.
impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
