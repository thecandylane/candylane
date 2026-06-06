//! candylane-core — the state engine, the Handler trait, and the three handlers.
//!
//! SCAFFOLD (Phase 1, Lane A): types + traits + engine orchestration are written.
//! Leaf I/O (SQLite store impl, handler impls, profile parse, reboot-pending probe)
//! is marked `todo!()` and lands in Lanes B/C/D/E. NOT yet compiled (no toolchain here).

pub mod engine;
pub mod handler;
pub mod store;
pub mod types;

// pub mod profile;            // Lane A tail: TOML parse → Vec<Item>
// pub mod handlers { pub mod winget; pub mod dotfile; pub mod script; }  // Lanes B/C/D

pub use handler::{Handler, RawOutput, WingetExecutor};
pub use types::*;

/// Crate-wide result. Swap to a `thiserror` enum once error taxonomy settles.
pub type Result<T> = anyhow::Result<T>;
