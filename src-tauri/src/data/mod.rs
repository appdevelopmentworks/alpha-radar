//! Data layer (P0/P4): CSV parse, Python sidecar client, SQLite cache
//! (differential update), and symbol-universe management. See docs/04-data-spec.md.

pub mod cache;
pub mod csv;
pub mod sidecar;
pub mod universe;
